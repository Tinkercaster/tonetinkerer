//! "Sustain" compressor: a smooth, optical-flavoured feed-forward compressor
//! placed before the drive and amp so that sustained notes keep pushing the
//! amp into saturation as they decay, the classic singing-lead recipe.

use crate::util::{Smoothed, db_to_gain, undenormal};

pub struct SustainCompressor {
    sample_rate: f32,
    env_db: f32,
    attack: f32,
    release_fast: f32,
    release_slow: f32,
    slow: f32,
    amount: Smoothed,
    gr_db: f32,
}

impl SustainCompressor {
    pub fn new(sample_rate: f32) -> Self {
        let mut amount = Smoothed::new(0.0);
        amount.set_time(sample_rate, 30.0);
        Self {
            sample_rate,
            env_db: -120.0,
            attack: coeff(sample_rate, 4.0),
            release_fast: coeff(sample_rate, 90.0),
            release_slow: coeff(sample_rate, 900.0),
            slow: 0.0,
            amount,
            gr_db: 0.0,
        }
    }

    pub fn reset(&mut self, amount: f32) {
        self.env_db = -120.0;
        self.slow = 0.0;
        self.gr_db = 0.0;
        self.amount.reset(amount);
    }

    /// Gain reduction of the last processed sample in dB (positive number).
    pub fn gain_reduction_db(&self) -> f32 {
        self.gr_db
    }

    /// `amount` in 0..=1: 0 is effectively off, 1 is heavy squash.
    pub fn process(&mut self, buf: &mut [f32], amount: f32) {
        self.amount.set_target(amount.clamp(0.0, 1.0));
        let _ = self.sample_rate;
        for x in buf.iter_mut() {
            let s = self.amount.next();
            // Threshold sweeps from -8 dB down to -42 dB, ratio from 1.5:1 to 10:1.
            let threshold = -8.0 - 34.0 * s;
            let ratio = 1.5 + 8.5 * s * s;
            let knee = 8.0;

            let level_db = 20.0 * x.abs().max(1e-6).log10();
            // Program-dependent release: a slow follower stretches release
            // after long notes, like an opto cell.
            let rel = self.release_fast + (self.release_slow - self.release_fast) * self.slow;
            let c = if level_db > self.env_db {
                self.attack
            } else {
                rel
            };
            self.env_db = level_db + (self.env_db - level_db) * c;
            self.slow = undenormal(
                self.slow * 0.9999 + 0.0001 * if self.env_db > threshold { 1.0 } else { 0.0 },
            );

            let over = self.env_db - threshold;
            let gr = if over <= -knee * 0.5 {
                0.0
            } else if over >= knee * 0.5 {
                over * (1.0 - 1.0 / ratio)
            } else {
                let t = over + knee * 0.5;
                (1.0 - 1.0 / ratio) * t * t / (2.0 * knee)
            };
            self.gr_db = gr;
            // Automatic make-up: restore roughly half the static reduction at
            // the threshold so the knob reads as "more sustain", not "quieter".
            let makeup = (-threshold) * (1.0 - 1.0 / ratio) * 0.5;
            *x *= db_to_gain(makeup - gr);
        }
    }
}

fn coeff(sample_rate: f32, ms: f32) -> f32 {
    (-1.0 / (sample_rate * ms * 0.001).max(1.0)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn reduces_dynamic_range() {
        let sr = 48_000.0;
        let mut c = SustainCompressor::new(sr);
        c.reset(0.8);
        let loud: Vec<f32> = (0..24_000).map(|i| 0.5 * (i as f32 * 0.03).sin()).collect();
        let quiet: Vec<f32> = loud.iter().map(|v| v * 0.05).collect();
        let mut a = loud.clone();
        let mut b = quiet.clone();
        c.process(&mut a, 0.8);
        c.reset(0.8);
        c.process(&mut b, 0.8);
        let in_ratio = rms(&loud) / rms(&quiet);
        let out_ratio = rms(&a[12_000..]) / rms(&b[12_000..]);
        assert!(out_ratio < in_ratio * 0.5, "in {in_ratio} out {out_ratio}");
    }
}
