//! Stereo echo: two modulated delay lines with an analog-style feedback loop
//! (band-limited, gently saturating), optional ping-pong cross-feedback and
//! input-envelope ducking so repeats bloom in the gaps between phrases.
//! Delay-time changes glide (tape-like) instead of clicking.

use crate::util::{Smoothed, undenormal};
use std::f32::consts::PI;

pub const MAX_DELAY_SECONDS: f32 = 2.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelaySettings {
    pub time_ms: f32,
    pub feedback: f32,
    pub mix: f32,
    /// 0 = dark (1.2 kHz), 1 = bright (12 kHz) repeats.
    pub tone: f32,
    pub ping_pong: bool,
    /// 0 = no ducking, 1 = repeats fully ducked while playing.
    pub duck: f32,
    /// 0..1 depth of the slow pitch modulation on the repeats.
    pub modulation: f32,
}

impl Default for DelaySettings {
    fn default() -> Self {
        Self {
            time_ms: 400.0,
            feedback: 0.35,
            mix: 0.25,
            tone: 0.5,
            ping_pong: true,
            duck: 0.0,
            modulation: 0.3,
        }
    }
}

struct Line {
    buf: Vec<f32>,
    w: usize,
    lp: f32,
    hp: f32,
}

impl Line {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len],
            w: 0,
            lp: 0.0,
            hp: 0.0,
        }
    }
    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.w = 0;
        self.lp = 0.0;
        self.hp = 0.0;
    }
    /// 4-point Hermite read `delay` samples behind the write head.
    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let len = self.buf.len();
        let d = delay.clamp(1.0, (len - 3) as f32);
        let di = d.floor() as usize;
        let f = d - di as f32;
        let idx = |k: usize| self.buf[(self.w + len * 2 - di - k) % len];
        // Samples at delays di-1, di, di+1, di+2.
        let (xm1, x0, x1, x2) = (
            self.buf[(self.w + len * 2 - di + 1) % len],
            idx(0),
            idx(1),
            idx(2),
        );
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + x0
    }
    #[inline]
    fn write(&mut self, x: f32) {
        self.buf[self.w] = x;
        self.w = (self.w + 1) % self.buf.len();
    }
}

pub struct StereoDelay {
    sr: f32,
    l: Line,
    r: Line,
    time: Smoothed,
    feedback: Smoothed,
    mix: Smoothed,
    lfo: f32,
    env: f32,
    duck_gain: f32,
}

impl StereoDelay {
    pub fn new(sr: f32) -> Self {
        let len = (MAX_DELAY_SECONDS * sr) as usize + 8;
        let mut time = Smoothed::new(400.0);
        time.set_time(sr, 120.0);
        let mut feedback = Smoothed::new(0.35);
        feedback.set_time(sr, 30.0);
        let mut mix = Smoothed::new(0.25);
        mix.set_time(sr, 30.0);
        Self {
            sr,
            l: Line::new(len),
            r: Line::new(len),
            time,
            feedback,
            mix,
            lfo: 0.0,
            env: 0.0,
            duck_gain: 1.0,
        }
    }

    pub fn reset(&mut self, s: &DelaySettings) {
        self.l.reset();
        self.r.reset();
        self.time
            .reset(s.time_ms.clamp(1.0, MAX_DELAY_SECONDS * 1000.0 - 50.0));
        self.feedback.reset(s.feedback);
        self.mix.reset(s.mix);
        self.env = 0.0;
        self.duck_gain = 1.0;
        // Restart the modulation so a reset matches a freshly built engine.
        self.lfo = 0.0;
    }

    /// Add the echo of `mono` (the feed, already scaled by the bypass fade so
    /// trails ring out when the delay is switched off) onto `left`/`right`.
    /// Returns the peak wet level of this block (for tail detection).
    pub fn process(
        &mut self,
        mono: &[f32],
        left: &mut [f32],
        right: &mut [f32],
        s: &DelaySettings,
    ) -> f32 {
        self.time
            .set_target(s.time_ms.clamp(1.0, MAX_DELAY_SECONDS * 1000.0 - 50.0));
        self.feedback.set_target(s.feedback.clamp(0.0, 0.98));
        self.mix.set_target(s.mix.clamp(0.0, 1.0));
        let cutoff = 1_200.0 * 10f32.powf(s.tone.clamp(0.0, 1.0));
        let lp_c = 1.0 - (-2.0 * PI * cutoff / self.sr).exp();
        let hp_c = 1.0 - (-2.0 * PI * 90.0 / self.sr).exp();
        let lfo_inc = 2.0 * PI * 0.35 / self.sr;
        let mod_depth = s.modulation.clamp(0.0, 1.0) * 0.0018 * self.sr; // up to 1.8 ms
        let env_rel = (-1.0 / (0.25 * self.sr)).exp();
        let duck_c = (-1.0 / (0.04 * self.sr)).exp();
        let mut peak = 0.0f32;

        for i in 0..mono.len() {
            let x = mono[i];
            let base = self.time.next() * 0.001 * self.sr;
            let fb = self.feedback.next();
            let mix = self.mix.next();
            self.lfo += lfo_inc;
            if self.lfo > 2.0 * PI {
                self.lfo -= 2.0 * PI;
            }
            let m = self.lfo.sin() * mod_depth;
            let (dl, dr) = if s.ping_pong {
                (base + m, base - m)
            } else {
                (base + m, base * 1.0 + 0.011 * self.sr - m)
            };
            let yl = self.l.read(dl);
            let yr = self.r.read(dr);

            // Loop filtering: band-limit and softly saturate each repeat.
            let shape = |line: &mut Line, y: f32| -> f32 {
                line.lp += (y - line.lp) * lp_c;
                line.hp += (line.lp - line.hp) * hp_c;
                let v = line.lp - line.hp;
                line.lp = undenormal(line.lp);
                line.hp = undenormal(line.hp);
                v.tanh()
            };
            let fl = shape(&mut self.l, yl);
            let fr = shape(&mut self.r, yr);

            let (in_l, in_r) = if s.ping_pong {
                (x + fr * fb, fl * fb)
            } else {
                (x + fl * fb, x + fr * fb)
            };
            self.l.write(in_l);
            self.r.write(in_r);

            // Ducking follows the dry playing level.
            let a = x.abs();
            self.env = if a > self.env {
                a
            } else {
                a + (self.env - a) * env_rel
            };
            let target = 1.0 - s.duck.clamp(0.0, 1.0) * (self.env * 12.0).min(1.0);
            self.duck_gain = target + (self.duck_gain - target) * duck_c;

            let g = mix * self.duck_gain;
            left[i] += yl * g;
            right[i] += yr * g;
            peak = peak.max(yl.abs()).max(yr.abs());
            if !left[i].is_finite() || !right[i].is_finite() {
                self.l.reset();
                self.r.reset();
                left[i] = 0.0;
                right[i] = 0.0;
            }
        }
        peak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_echo_arrives_on_time() {
        let sr = 48_000.0;
        let mut d = StereoDelay::new(sr);
        let s = DelaySettings {
            time_ms: 250.0,
            feedback: 0.0,
            mix: 1.0,
            modulation: 0.0,
            ping_pong: true,
            ..Default::default()
        };
        d.reset(&s);
        let n = 20_000;
        let mut mono = vec![0.0; n];
        mono[100] = 1.0;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        d.process(&mono, &mut l, &mut r, &s);
        let (imax, _) = l.iter().enumerate().fold((0, 0.0f32), |(bi, bv), (i, &v)| {
            if v.abs() > bv { (i, v.abs()) } else { (bi, bv) }
        });
        assert!((imax as i32 - (100 + 12_000)).abs() <= 2, "echo at {imax}");
        // Ping-pong with zero feedback: nothing reaches the right side.
        assert!(r.iter().all(|v| v.abs() < 1e-6));
    }

    #[test]
    fn high_feedback_stays_bounded() {
        let sr = 48_000.0;
        let mut d = StereoDelay::new(sr);
        let s = DelaySettings {
            time_ms: 50.0,
            feedback: 0.98,
            mix: 1.0,
            ..Default::default()
        };
        d.reset(&s);
        let mono: Vec<f32> = (0..sr as usize * 5)
            .map(|i| if i < 2000 { 0.9 } else { 0.0 })
            .collect();
        let mut l = vec![0.0; mono.len()];
        let mut r = vec![0.0; mono.len()];
        d.process(&mono, &mut l, &mut r, &s);
        assert!(
            l.iter()
                .chain(r.iter())
                .all(|v| v.is_finite() && v.abs() < 1.5)
        );
    }
}
