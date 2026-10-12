//! Stereo chorus: one delay line read by two modulated taps (quadrature
//! LFOs, ~6-14 ms), one per side, for wide 80s cleans, funk and ambient.

use crate::util::{Smoothed, undenormal};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChorusSettings {
    pub rate_hz: f32,
    /// 0..1 modulation depth (up to +-4 ms).
    pub depth: f32,
    /// 0..1 wet amount.
    pub mix: f32,
}

impl Default for ChorusSettings {
    fn default() -> Self {
        Self {
            rate_hz: 0.8,
            depth: 0.5,
            mix: 0.5,
        }
    }
}

pub struct Chorus {
    sr: f32,
    buf: Vec<f32>,
    w: usize,
    phase: f32,
    rate: Smoothed,
    depth: Smoothed,
    mix: Smoothed,
    hp: f32,
}

const BASE_MS: f32 = 10.0;
const MAX_MOD_MS: f32 = 4.0;

impl Chorus {
    pub fn new(sr: f32) -> Self {
        let len = (sr * (BASE_MS + MAX_MOD_MS + 2.0) * 0.001) as usize + 4;
        let mk = |v: f32| {
            let mut s = Smoothed::new(v);
            s.set_time(sr, 30.0);
            s
        };
        Self {
            sr,
            buf: vec![0.0; len],
            w: 0,
            phase: 0.0,
            rate: mk(0.8),
            depth: mk(0.5),
            mix: mk(0.5),
            hp: 0.0,
        }
    }

    pub fn reset(&mut self, s: &ChorusSettings) {
        self.buf.fill(0.0);
        self.w = 0;
        self.phase = 0.0;
        self.hp = 0.0;
        self.rate.reset(s.rate_hz);
        self.depth.reset(s.depth);
        self.mix.reset(s.mix);
    }

    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let len = self.buf.len();
        let d = delay.clamp(1.0, (len - 3) as f32);
        let di = d.floor() as usize;
        let f = d - di as f32;
        let at = |k: usize| self.buf[(self.w + len * 2 - di - k) % len];
        let (xm1, x0, x1, x2) = (
            self.buf[(self.w + len * 2 - di + 1) % len],
            at(0),
            at(1),
            at(2),
        );
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + x0
    }

    /// Replace `left`/`right` (which hold the dry signal) with the chorused
    /// signal, crossfaded by `amount` (bypass fade; one value per sample).
    pub fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        s: &ChorusSettings,
        amount: &[f32],
    ) {
        self.rate.set_target(s.rate_hz.clamp(0.05, 8.0));
        self.depth.set_target(s.depth.clamp(0.0, 1.0));
        self.mix.set_target(s.mix.clamp(0.0, 1.0));
        let hp_c = 1.0 - (-2.0 * PI * 120.0 / self.sr).exp();
        for i in 0..left.len() {
            let dry = 0.5 * (left[i] + right[i]);
            // Keep the very low end out of the modulated path (no wobbly bass).
            self.hp = undenormal(self.hp + (dry - self.hp) * hp_c);
            let x = dry - self.hp;
            self.buf[self.w] = x;
            self.w = (self.w + 1) % self.buf.len();

            self.phase += 2.0 * PI * self.rate.next() / self.sr;
            if self.phase > 2.0 * PI {
                self.phase -= 2.0 * PI;
            }
            let depth = self.depth.next() * MAX_MOD_MS * 0.001 * self.sr;
            let base = BASE_MS * 0.001 * self.sr;
            let wl = self.read(base + depth * self.phase.sin());
            let wr = self.read(base + depth * (self.phase + 0.5 * PI).sin());
            let mix = self.mix.next();
            let a = amount[i];
            let ol = left[i] * (1.0 - 0.35 * mix) + wl * mix * 0.85;
            let or = right[i] * (1.0 - 0.35 * mix) + wr * mix * 0.85;
            left[i] += (ol - left[i]) * a;
            right[i] += (or - right[i]) * a;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widens_a_mono_signal() {
        let sr = 48_000.0;
        let mut c = Chorus::new(sr);
        let s = ChorusSettings {
            rate_hz: 1.0,
            depth: 0.8,
            mix: 0.6,
        };
        c.reset(&s);
        let n = 48_000;
        let x: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / sr).sin() * 0.3)
            .collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        c.process(&mut l, &mut r, &s, &vec![1.0; n]);
        let side: f32 = l
            .iter()
            .zip(&r)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f32>()
            / n as f32;
        assert!(side.sqrt() > 0.02, "no stereo width");
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 1.0));
    }

    #[test]
    fn zero_amount_is_bypass() {
        let sr = 48_000.0;
        let mut c = Chorus::new(sr);
        let s = ChorusSettings::default();
        c.reset(&s);
        let x: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.05).sin()).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        c.process(&mut l, &mut r, &s, &vec![0.0; 2000]);
        assert_eq!(l, x);
        assert_eq!(r, x);
    }
}
