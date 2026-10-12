//! Noise gate with hysteresis, hold, smooth gain changes and an automatic
//! threshold that follows the input's own background hiss.
//!
//! Detection runs on the raw guitar input; gain is applied to the same
//! signal before any gain stage so amplified hiss never gets a chance to build.
//!
//! Why the threshold follows the noise floor: every interface, guitar and
//! input level has a different hiss level, and the amps behind the gate lift
//! whatever passes by up to 70 dB. A fixed threshold is wrong both ways: set
//! high it chops notes that still ring loudly through a high-gain amp, set low
//! it never closes on a noisy input and the hiss washes through compressor and
//! reverb. So the gate measures the hiss and opens only clearly above it. The
//! sound's own threshold still applies when it is the higher of the two.

use crate::util::{db_to_gain, gain_to_db};

pub struct NoiseGate {
    sample_rate: f32,
    env: f32,
    env_release: f32,
    gain: f32,
    attack_coeff: f32,
    release_coeff: f32,
    hold_samples: u32,
    hold_counter: u32,
    open: bool,
    /// Mean-square level (short smoothing) for the noise-floor estimate.
    ms: f32,
    ms_coeff: f32,
    /// Hiss-or-note detector: low band (one-pole low-pass, 1.5 kHz) and top
    /// band (input minus a one-pole low-pass at 4 kHz) power, smoothed.
    lp_lo: f32,
    lp_hi: f32,
    a_lo: f32,
    a_hi: f32,
    lo_ms: f32,
    hi_ms: f32,
    /// RMS-style level the gate opens and closes on.
    level: f32,
    level_attack: f32,
    level_release: f32,
    /// Estimated background-noise power.
    floor: f32,
    floor_fall: f32,
    floor_rise_noise: f32,
    floor_rise_tonal: f32,
}

/// How far below the open threshold the signal must fall before closing.
const HYSTERESIS_DB: f32 = 5.0;
/// The gate opens this far above the measured hiss and closes 5 dB lower,
/// i.e. a note rings until it is within ~4 dB of the hiss (RMS). The level it
/// compares is an RMS reading (10 ms), on which hiss sits steady within
/// ~0.5 dB of its average, so these small margins don't chatter.
const FLOOR_MARGIN_DB: f32 = 9.0;
/// Starting guess for the hiss level, before anything has been measured.
const FLOOR_START_DB: f32 = -80.0;
/// Above this the "floor" is a played note, not hiss: never learn it.
const FLOOR_MAX_DB: f32 = -42.0;
/// Hiss is broadband, most of its power in the top octaves; a guitar note's
/// power sits low, even when quiet and with hiss on top. (Zero-crossing
/// counting is fooled: hiss adds crossings to a quiet note that still rings
/// well above it, and the gate then learned the note as hiss and cut it.)
/// The input sounds like hiss when the top band carries this many times
/// the low band's power (white noise: ~7x, a note with hiss 6 dB below it: <2x).
const HISS_TOP_TO_LOW: f32 = 2.0;

impl NoiseGate {
    pub fn new(sample_rate: f32) -> Self {
        let mut g = Self {
            sample_rate,
            env: 0.0,
            env_release: 0.0,
            gain: 0.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            hold_samples: 0,
            hold_counter: 0,
            open: false,
            ms: 0.0,
            ms_coeff: coeff(sample_rate, 30.0),
            lp_lo: 0.0,
            lp_hi: 0.0,
            a_lo: 1.0 - (-2.0 * std::f32::consts::PI * 1_500.0 / sample_rate).exp(),
            a_hi: 1.0 - (-2.0 * std::f32::consts::PI * 4_000.0 / sample_rate).exp(),
            lo_ms: 0.0,
            hi_ms: 0.0,
            level: 0.0,
            // Symmetric smoothing: on hiss the reading then sits within
            // ~0.5 dB of its true level (a faster attack than release
            // biases it upwards, and the gate lingered open on hiss).
            level_attack: coeff(sample_rate, 10.0),
            level_release: coeff(sample_rate, 10.0),
            floor: power(FLOOR_START_DB),
            // Falls fast (a quieter moment is always a better estimate) ...
            floor_fall: coeff(sample_rate, 60.0),
            // ... rises only while the input sounds like hiss: a ringing note
            // must never be learned (even a slow creep closed on long notes);
            // a hiss that really got louder is learned in the next pause.
            floor_rise_noise: rise(sample_rate, 60.0),
            floor_rise_tonal: 1.0,
        };
        // Close fast: the gain is applied before 60+ dB of amp gain, so a slow
        // fade lets the input's hiss through at full, distorted level after
        // every stop. 10 ms reaches -60 dB in ~70 ms; the hold keeps it from
        // chattering, and closing only just above the hiss keeps notes whole.
        g.set_release_ms(10.0);
        g.attack_coeff = coeff(sample_rate, 0.6);
        g.env_release = coeff(sample_rate, 25.0);
        g.hold_samples = (sample_rate * 0.04) as u32;
        g
    }

    pub fn set_release_ms(&mut self, ms: f32) {
        self.release_coeff = coeff(self.sample_rate, ms);
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
        self.gain = 0.0;
        self.hold_counter = 0;
        self.open = false;
        self.ms = 0.0;
        self.lp_lo = 0.0;
        self.lp_hi = 0.0;
        self.lo_ms = 0.0;
        self.hi_ms = 0.0;
        self.level = 0.0;
        self.floor = power(FLOOR_START_DB);
    }

    /// Current gate gain reduction in dB (0 = fully open). For metering.
    pub fn reduction_db(&self) -> f32 {
        gain_to_db(self.gain).max(-90.0)
    }

    /// Measured background-noise level (RMS dBFS). For metering.
    pub fn noise_floor_db(&self) -> f32 {
        10.0 * self.floor.max(1e-12).log10()
    }

    /// The level the gate opens at: the sound's threshold, or clearly above
    /// the measured hiss, whichever is higher.
    pub fn effective_threshold_db(&self, threshold_db: f32) -> f32 {
        threshold_db.max(self.noise_floor_db() + FLOOR_MARGIN_DB)
    }

    pub fn process(&mut self, buf: &mut [f32], threshold_db: f32) {
        // Thresholds per block: the floor moves slowly, a block is ~3 ms.
        let thr = self.effective_threshold_db(threshold_db);
        let open_thr = db_to_gain(thr);
        let close_thr = db_to_gain(thr - HYSTERESIS_DB);
        let floor_max = power(FLOOR_MAX_DB);
        let (open_ms, close_ms) = (open_thr * open_thr, close_thr * close_thr);
        for x in buf.iter_mut() {
            let a = x.abs();
            // Peak follower: instant attack, smooth release (metering only).
            self.env = if a > self.env {
                a
            } else {
                a + (self.env - a) * self.env_release
            };
            let sq = *x * *x;
            // RMS-style level for the gate decision.
            let c = if sq > self.level {
                self.level_attack
            } else {
                self.level_release
            };
            self.level = sq + (self.level - sq) * c;

            // Noise-floor estimate.
            self.ms = sq + (self.ms - sq) * self.ms_coeff;
            self.lp_lo += (*x - self.lp_lo) * self.a_lo;
            self.lp_hi += (*x - self.lp_hi) * self.a_hi;
            let top = *x - self.lp_hi;
            self.lo_ms = self.lp_lo * self.lp_lo + (self.lo_ms - self.lp_lo * self.lp_lo) * self.ms_coeff;
            self.hi_ms = top * top + (self.hi_ms - top * top) * self.ms_coeff;
            if self.ms < self.floor {
                self.floor = self.ms + (self.floor - self.ms) * self.floor_fall;
            } else {
                let r = if self.hi_ms > self.lo_ms * HISS_TOP_TO_LOW {
                    self.floor_rise_noise
                } else {
                    self.floor_rise_tonal
                };
                self.floor = (self.floor * r).min(self.ms).min(floor_max);
            }
            self.floor = self.floor.max(1e-12);

            if self.level >= open_ms {
                self.open = true;
                self.hold_counter = self.hold_samples;
            } else if self.level < close_ms {
                if self.hold_counter > 0 {
                    self.hold_counter -= 1;
                } else {
                    self.open = false;
                }
            }

            let target = if self.open { 1.0 } else { 0.0 };
            let c = if target > self.gain {
                self.attack_coeff
            } else {
                self.release_coeff
            };
            self.gain = target + (self.gain - target) * c;
            if self.gain < 1e-6 {
                self.gain = 0.0;
            }
            *x *= self.gain;
        }
    }
}

fn coeff(sample_rate: f32, ms: f32) -> f32 {
    (-1.0 / (sample_rate * ms * 0.001).max(1.0)).exp()
}

/// Power level for a dBFS value.
fn power(db: f32) -> f32 {
    10f32.powf(db / 10.0)
}

/// Per-sample power multiplier for a rise of `db_per_s`.
fn rise(sample_rate: f32, db_per_s: f32) -> f32 {
    10f32.powf(db_per_s / 10.0 / sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hiss(n: usize, db: f32, seed: u64) -> Vec<f32> {
        // Uniform white noise with the given RMS.
        let g = db_to_gain(db) * 3f32.sqrt();
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * g
            })
            .collect()
    }

    #[test]
    fn closes_on_noise_and_opens_on_signal() {
        let sr = 48_000.0;
        let mut g = NoiseGate::new(sr);
        // -70 dBFS noise-ish signal below a -50 dB threshold is attenuated hard.
        let mut quiet: Vec<f32> = (0..48_000)
            .map(|i| 3e-4 * ((i as f32 * 1.7).sin()))
            .collect();
        g.process(&mut quiet, -50.0);
        let tail_peak = quiet[40_000..].iter().fold(0.0_f32, |m, x| m.max(x.abs()));
        assert!(tail_peak < 1e-6, "gate leaked {tail_peak}");

        // A -12 dB note passes with unity gain after the attack.
        let mut loud: Vec<f32> = (0..4_800).map(|i| 0.25 * (i as f32 * 0.05).sin()).collect();
        let orig = loud.clone();
        g.process(&mut loud, -50.0);
        for i in 1_000..4_800 {
            assert!((loud[i] - orig[i]).abs() < 1e-4);
        }
    }

    /// Hiss louder than the sound's own threshold must still be shut off
    /// once the gate has heard it for a moment.
    #[test]
    fn learns_a_noisy_input_and_closes_on_it() {
        let sr = 48_000.0;
        for db in [-70.0, -60.0, -50.0] {
            let mut g = NoiseGate::new(sr);
            let mut x = hiss((sr * 4.0) as usize, db, 3);
            for c in x.chunks_mut(128) {
                g.process(c, -72.0);
            }
            let tail = &x[(sr * 3.0) as usize..];
            let pk = tail.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(pk < 1e-5, "hiss at {db} dBFS still passes ({pk})");
            assert!((g.noise_floor_db() - db).abs() < 4.0, "floor {} for {db}", g.noise_floor_db());
        }
    }

    /// A slowly fading string over quiet hiss must keep sounding until it is
    /// nearly down at the hiss, not be cut while it still rings.
    #[test]
    fn keeps_a_ringing_note_until_it_reaches_the_hiss() {
        let sr = 48_000.0;
        let n = (sr * 12.0) as usize;
        let noise = hiss(n, -80.0, 5);
        // 0.5 s of hiss alone, then a 196 Hz note fading at 6 dB/s from -9 dBFS.
        let start = (sr * 0.5) as usize;
        let mut x = noise.clone();
        for i in start..n {
            let t = (i - start) as f32 / sr;
            x[i] += db_to_gain(-9.0 - 6.0 * t) * (2.0 * std::f32::consts::PI * 196.0 * t).sin();
        }
        let orig = x.clone();
        let mut g = NoiseGate::new(sr);
        for c in x.chunks_mut(128) {
            g.process(c, -66.0);
        }
        // The note is 20 dB above the hiss (-60 dBFS peak) after 8.5 s: until
        // then the gate must pass it untouched.
        let until = start + (sr * 8.5) as usize;
        for i in (start + 480)..until {
            assert!(
                (x[i] - orig[i]).abs() < 1e-4,
                "note cut at {:.2} s ({:.0} dBFS)",
                (i - start) as f32 / sr,
                -9.0 - 6.0 * (i - start) as f32 / sr
            );
        }
    }
}
