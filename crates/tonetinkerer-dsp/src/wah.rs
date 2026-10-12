//! Wah filter: a resonant state-variable filter swept between ~350 Hz and
//! ~2.3 kHz, either from a manual position (automate it like an expression
//! pedal) or from the picking envelope (auto-wah / envelope filter).

use crate::util::{Smoothed, undenormal};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WahMode {
    #[default]
    Off,
    Manual,
    Auto,
}

pub struct Wah {
    sample_rate: f32,
    ic1: f32,
    ic2: f32,
    pos: Smoothed,
    env: f32,
    env_att: f32,
    env_rel: f32,
}

const F_MIN: f32 = 350.0;
const F_MAX: f32 = 2300.0;
const Q: f32 = 5.0;

impl Wah {
    pub fn new(sample_rate: f32) -> Self {
        let mut pos = Smoothed::new(0.5);
        pos.set_time(sample_rate, 15.0);
        Self {
            sample_rate,
            ic1: 0.0,
            ic2: 0.0,
            pos,
            env: 0.0,
            env_att: (-1.0 / (sample_rate * 0.004)).exp(),
            env_rel: (-1.0 / (sample_rate * 0.18)).exp(),
        }
    }

    pub fn reset(&mut self, position: f32) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
        self.env = 0.0;
        self.pos.reset(position);
    }

    /// Process in place. Returns the wet signal only; the chain crossfades
    /// it against the dry signal for click-free mode switching.
    pub fn process(&mut self, buf: &mut [f32], mode: WahMode, position: f32, sensitivity: f32) {
        self.pos.set_target(position.clamp(0.0, 1.0));
        let k = 1.0 / Q;
        for x in buf.iter_mut() {
            let input = *x;
            let base = self.pos.next();
            let p = match mode {
                WahMode::Auto => {
                    let a = input.abs();
                    let c = if a > self.env {
                        self.env_att
                    } else {
                        self.env_rel
                    };
                    self.env = undenormal(a + (self.env - a) * c);
                    // Sensitivity scales how far the envelope opens the filter.
                    let drive = (self.env * (4.0 + 60.0 * sensitivity)).min(1.0);
                    (base * 0.6 + drive * (1.0 - base * 0.6)).clamp(0.0, 1.0)
                }
                _ => base,
            };
            let freq = F_MIN * (F_MAX / F_MIN).powf(p);
            let g = (PI * freq / self.sample_rate).tan();
            let a1 = 1.0 / (1.0 + g * (g + k));
            let a2 = g * a1;
            let a3 = g * a2;
            // Zavalishin TPT SVF.
            let v3 = input - self.ic2;
            let v1 = a1 * self.ic1 + a2 * v3;
            let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
            self.ic1 = undenormal(2.0 * v1 - self.ic1);
            self.ic2 = undenormal(2.0 * v2 - self.ic2);
            let bp = v1;
            let lp = v2;
            // Inductor-wah character: mostly band-pass with some low-pass body.
            *x = 2.2 * bp * k * 2.5 + 0.35 * lp;
            if !x.is_finite() {
                self.ic1 = 0.0;
                self.ic2 = 0.0;
                *x = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_gain(pos: f32, freq: f32) -> f32 {
        let sr = 48_000.0;
        let mut w = Wah::new(sr);
        w.reset(pos);
        let mut b: Vec<f32> = (0..24_000)
            .map(|i| (2.0 * PI * freq * i as f32 / sr).sin())
            .collect();
        w.process(&mut b, WahMode::Manual, pos, 0.0);
        b[12_000..].iter().fold(0.0_f32, |m, v| m.max(v.abs()))
    }

    #[test]
    fn sweep_moves_the_resonance() {
        // Heel down favours lows, toe down favours highs.
        assert!(tone_gain(0.0, 400.0) > tone_gain(0.0, 2000.0) * 2.0);
        assert!(tone_gain(1.0, 2000.0) > tone_gain(1.0, 400.0) * 2.0);
    }
}
