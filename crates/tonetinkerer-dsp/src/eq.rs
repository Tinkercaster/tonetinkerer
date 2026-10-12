//! Post-cab tone EQ: bass shelf, sweepable mid peak, treble shelf and a
//! presence peak. Gains are smoothed per sample and coefficients refreshed
//! every 16 samples while anything is moving.

use crate::biquad::{Biquad, Coeffs};
use crate::util::Smoothed;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqSettings {
    pub bass_db: f32,
    pub mid_db: f32,
    pub mid_freq: f32,
    pub treble_db: f32,
    pub presence_db: f32,
    /// Low-mid "body" peak at 300 Hz (driven mostly by the Body macro).
    pub body_db: f32,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            bass_db: 0.0,
            mid_db: 0.0,
            mid_freq: 800.0,
            treble_db: 0.0,
            presence_db: 0.0,
            body_db: 0.0,
        }
    }
}

pub struct Eq {
    sr: f32,
    bands: [Biquad; 5],
    s: [Smoothed; 6],
    counter: u32,
}

impl Eq {
    pub fn new(sr: f32) -> Self {
        let d = EqSettings::default();
        let mut s = [
            Smoothed::new(d.bass_db),
            Smoothed::new(d.mid_db),
            Smoothed::new(d.mid_freq),
            Smoothed::new(d.treble_db),
            Smoothed::new(d.presence_db),
            Smoothed::new(d.body_db),
        ];
        for x in s.iter_mut() {
            x.set_time(sr, 25.0);
        }
        let mut eq = Self {
            sr,
            bands: [Biquad::default(); 5],
            s,
            counter: 0,
        };
        eq.update();
        eq
    }

    fn update(&mut self) {
        let v: [f32; 6] = std::array::from_fn(|i| self.s[i].value());
        self.bands[0].c = Coeffs::low_shelf(self.sr, 110.0, 0.7, v[0]);
        self.bands[1].c = Coeffs::peaking(self.sr, v[2], 0.9, v[1]);
        self.bands[2].c = Coeffs::high_shelf(self.sr, 2_800.0, 0.7, v[3]);
        self.bands[3].c = Coeffs::peaking(self.sr, 4_800.0, 1.2, v[4]);
        self.bands[4].c = Coeffs::peaking(self.sr, 300.0, 0.8, v[5]);
    }

    fn targets(e: &EqSettings) -> [f32; 6] {
        [
            e.bass_db,
            e.mid_db,
            e.mid_freq.clamp(200.0, 4_000.0),
            e.treble_db,
            e.presence_db,
            e.body_db,
        ]
    }

    pub fn reset(&mut self, e: &EqSettings) {
        for (s, t) in self.s.iter_mut().zip(Self::targets(e)) {
            s.reset(t);
        }
        for b in self.bands.iter_mut() {
            b.reset();
        }
        self.update();
    }

    pub fn process(&mut self, buf: &mut [f32], e: &EqSettings) {
        for (s, t) in self.s.iter_mut().zip(Self::targets(e)) {
            s.set_target(t);
        }
        for x in buf.iter_mut() {
            let mut moving = false;
            for s in self.s.iter_mut() {
                s.next();
                moving |= !s.is_settled();
            }
            self.counter += 1;
            if self.counter >= 16 {
                self.counter = 0;
                if moving {
                    self.update();
                }
            }
            let mut y = *x;
            for b in self.bands.iter_mut() {
                y = b.process(y);
            }
            *x = y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_eq_is_transparent() {
        let mut eq = Eq::new(48_000.0);
        let e = EqSettings::default();
        eq.reset(&e);
        let x: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut y = x.clone();
        eq.process(&mut y, &e);
        for i in 0..2000 {
            assert!((x[i] - y[i]).abs() < 1e-4);
        }
    }

    #[test]
    fn mid_boost_boosts() {
        let sr = 48_000.0;
        let mut eq = Eq::new(sr);
        let e = EqSettings {
            mid_db: 9.0,
            mid_freq: 1000.0,
            ..Default::default()
        };
        eq.reset(&e);
        let x: Vec<f32> = (0..24_000)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr).sin() * 0.1)
            .collect();
        let mut y = x.clone();
        eq.process(&mut y, &e);
        let peak = y[12_000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((20.0 * (peak / 0.1).log10() - 9.0).abs() < 0.5);
    }
}
