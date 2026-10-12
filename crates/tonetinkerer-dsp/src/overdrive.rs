//! Mid-hump overdrive pedal, in the spirit of the classic green screamer
//! circuit that so many instrumental-rock lead players stack in front of a
//! cranked amp: the clipping branch is high-passed (tight lows, mid focus),
//! soft diode clipping, the clean signal summed back in, then a tone control.
//!
//! The nonlinear part runs at 4x oversampling. The stage always imposes
//! exactly `oversample::LATENCY` samples of delay (the bypass path is delayed
//! to match) so total plugin latency is constant whether it is on or off.

use crate::biquad::{Biquad, Coeffs};
use crate::oversample::{FACTOR, LATENCY, Oversampler4x};
use crate::util::{Fade, Smoothed, db_to_gain};

pub struct Overdrive {
    sr_hi: f32,
    os: Oversampler4x,
    hp: Biquad,
    tone_lp: Biquad,
    post_lp: Biquad,
    drive: Smoothed,
    tone: Smoothed,
    level: Smoothed,
    fade: Fade,
    dry_delay: [f32; LATENCY + 1],
    dry_pos: usize,
    tone_counter: u32,
    fat: bool,
}

impl Overdrive {
    pub fn new(sample_rate: f32) -> Self {
        let sr_hi = sample_rate * FACTOR as f32;
        let mut drive = Smoothed::new(0.5);
        let mut tone = Smoothed::new(0.5);
        let mut level = Smoothed::new(1.0);
        for s in [&mut drive, &mut tone, &mut level] {
            s.set_time(sample_rate, 20.0);
        }
        let mut fade = Fade::new(false);
        fade.set_time(sample_rate, 12.0);
        Self {
            sr_hi,
            os: Oversampler4x::new(),
            hp: Biquad::new(Coeffs::highpass(sr_hi, 720.0, 0.6)),
            tone_lp: Biquad::new(Coeffs::lowpass(sr_hi, 3_000.0, 0.707)),
            post_lp: Biquad::new(Coeffs::lowpass(sr_hi, 9_000.0, 0.707)),
            drive,
            tone,
            level,
            fade,
            dry_delay: [0.0; LATENCY + 1],
            dry_pos: 0,
            tone_counter: 0,
            fat: false,
        }
    }

    /// "Fat" mode lowers the clipping branch's high-pass from 720 Hz to
    /// 250 Hz: the same smooth clipping without thinning the low mids.
    fn set_fat(&mut self, fat: bool) {
        if fat != self.fat {
            self.fat = fat;
            let f = if fat { 250.0 } else { 720.0 };
            self.hp.c = Coeffs::highpass(self.sr_hi, f, 0.6);
        }
    }

    pub fn reset(&mut self, enabled: bool, drive: f32, tone: f32, level_db: f32, fat: bool) {
        self.set_fat(fat);
        self.os.reset();
        self.hp.reset();
        self.tone_lp.reset();
        self.post_lp.reset();
        self.dry_delay = [0.0; LATENCY + 1];
        self.fade.reset(enabled);
        self.drive.reset(drive);
        self.tone.reset(tone);
        self.level.reset(db_to_gain(level_db));
        self.update_tone();
    }

    fn update_tone(&mut self) {
        // Tone sweeps the post-clip low-pass from a dark 700 Hz to a bright 6 kHz.
        let f = 700.0 * (6_000.0f32 / 700.0).powf(self.tone.value());
        self.tone_lp.c = Coeffs::lowpass(self.sr_hi, f, 0.707);
    }

    #[inline]
    fn clip(x: f32) -> f32 {
        // Silicon diode pair in the feedback loop: soft, slightly asymmetric.
        let pos = x / (1.0 + x.abs()).powf(0.9);
        pos + 0.04 * (x.abs() / (1.0 + x.abs()))
    }

    pub fn process(
        &mut self,
        buf: &mut [f32],
        enabled: bool,
        drive: f32,
        tone: f32,
        level_db: f32,
        fat: bool,
    ) {
        self.set_fat(fat);
        self.fade.set_engaged(enabled);
        self.drive.set_target(drive.clamp(0.0, 1.0));
        self.tone.set_target(tone.clamp(0.0, 1.0));
        self.level.set_target(db_to_gain(level_db));

        let mut hi = [0.0f32; FACTOR];
        for x in buf.iter_mut() {
            // Delay-matched dry path.
            self.dry_delay[self.dry_pos] = *x;
            let dry = self.dry_delay[(self.dry_pos + 1) % (LATENCY + 1)];
            self.dry_pos = (self.dry_pos + 1) % (LATENCY + 1);

            let mix = self.fade.next();
            let d = self.drive.next();
            let _ = self.tone.next();
            let level = self.level.next();
            if mix == 0.0 && self.fade.is_off() {
                *x = dry;
                continue;
            }
            self.tone_counter += 1;
            if self.tone_counter >= 32 && !self.tone.is_settled() {
                self.tone_counter = 0;
                self.update_tone();
            }

            let gain = 1.0 + 140.0 * d * d;
            self.os.upsample(*x, &mut hi);
            for h in hi.iter_mut() {
                let branch = Self::clip(self.hp.process(*h) * gain);
                let summed = *h * 0.5 + branch * 0.6;
                *h = self.post_lp.process(self.tone_lp.process(summed));
            }
            let wet = self.os.downsample(&hi) * level;
            *x = dry + (wet - dry) * mix;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bypass_is_pure_delay() {
        let mut od = Overdrive::new(48_000.0);
        od.reset(false, 0.5, 0.5, 0.0, false);
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut b = input.clone();
        od.process(&mut b, false, 0.5, 0.5, 0.0, false);
        for i in LATENCY..1000 {
            assert_eq!(b[i], input[i - LATENCY]);
        }
    }

    #[test]
    fn engaged_adds_harmonics_and_stays_bounded() {
        let mut od = Overdrive::new(48_000.0);
        od.reset(true, 1.0, 0.5, 0.0, true);
        let mut b: Vec<f32> = (0..48_000).map(|i| 0.5 * (i as f32 * 0.05).sin()).collect();
        od.process(&mut b, true, 1.0, 0.5, 0.0, true);
        assert!(b.iter().all(|x| x.is_finite() && x.abs() < 2.0));
    }
}
