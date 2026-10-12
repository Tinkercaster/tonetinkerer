//! RBJ "Audio EQ Cookbook" biquads in transposed direct form II.
//!
//! Coefficients are designed in f32 but the filter runs in f64: low cutoffs
//! at high sample rates put the poles very close to the unit circle, where an
//! f32 TDF-II amplifies its own rounding error (audible as low-frequency
//! noise, and measurable as block-size dependent output).

use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Coeffs {
    pub const IDENTITY: Coeffs = Coeffs {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    fn normalize(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        let inv = 1.0 / a0;
        Self {
            b0: b0 * inv,
            b1: b1 * inv,
            b2: b2 * inv,
            a1: a1 * inv,
            a2: a2 * inv,
        }
    }

    fn omega(sr: f32, freq: f32) -> (f32, f32) {
        // Keep the corner safely below Nyquist whatever the host rate is.
        let f = freq.clamp(1.0, sr * 0.49);
        let w = 2.0 * PI * f / sr;
        (w.cos(), w.sin())
    }

    pub fn lowpass(sr: f32, freq: f32, q: f32) -> Self {
        let (c, s) = Self::omega(sr, freq);
        let alpha = s / (2.0 * q);
        Self::normalize(
            (1.0 - c) / 2.0,
            1.0 - c,
            (1.0 - c) / 2.0,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    pub fn highpass(sr: f32, freq: f32, q: f32) -> Self {
        let (c, s) = Self::omega(sr, freq);
        let alpha = s / (2.0 * q);
        Self::normalize(
            (1.0 + c) / 2.0,
            -(1.0 + c),
            (1.0 + c) / 2.0,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    pub fn peaking(sr: f32, freq: f32, q: f32, gain_db: f32) -> Self {
        let (c, s) = Self::omega(sr, freq);
        let a = 10.0_f32.powf(gain_db / 40.0);
        let alpha = s / (2.0 * q);
        Self::normalize(
            1.0 + alpha * a,
            -2.0 * c,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * c,
            1.0 - alpha / a,
        )
    }

    pub fn low_shelf(sr: f32, freq: f32, q: f32, gain_db: f32) -> Self {
        let (c, s) = Self::omega(sr, freq);
        let a = 10.0_f32.powf(gain_db / 40.0);
        let alpha = s / (2.0 * q);
        let sa = 2.0 * a.sqrt() * alpha;
        Self::normalize(
            a * ((a + 1.0) - (a - 1.0) * c + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - sa),
            (a + 1.0) + (a - 1.0) * c + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - sa,
        )
    }

    pub fn high_shelf(sr: f32, freq: f32, q: f32, gain_db: f32) -> Self {
        let (c, s) = Self::omega(sr, freq);
        let a = 10.0_f32.powf(gain_db / 40.0);
        let alpha = s / (2.0 * q);
        let sa = 2.0 * a.sqrt() * alpha;
        Self::normalize(
            a * ((a + 1.0) + (a - 1.0) * c + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - sa),
            (a + 1.0) - (a - 1.0) * c + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - sa,
        )
    }

    /// Magnitude response at `freq` (used by tests and the GUI-free analysis tools).
    pub fn magnitude(&self, sr: f32, freq: f32) -> f32 {
        let w = 2.0 * PI * freq / sr;
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    pub c: Coeffs,
    z1: f64,
    z2: f64,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            c: Coeffs::IDENTITY,
            z1: 0.0,
            z2: 0.0,
        }
    }
}

impl Biquad {
    pub fn new(c: Coeffs) -> Self {
        Self {
            c,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let c = &self.c;
        let x = x as f64;
        let y = c.b0 as f64 * x + self.z1;
        self.z1 = c.b1 as f64 * x - c.a1 as f64 * y + self.z2;
        self.z2 = c.b2 as f64 * x - c.a2 as f64 * y;
        if !y.is_finite() {
            self.reset();
            return 0.0;
        }
        y as f32
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SR: f32 = 48_000.0;

    fn db(x: f32) -> f32 {
        20.0 * x.log10()
    }

    #[test]
    fn lowpass_response() {
        let c = Coeffs::lowpass(SR, 1000.0, std::f32::consts::FRAC_1_SQRT_2);
        assert!(db(c.magnitude(SR, 50.0)).abs() < 0.1);
        assert!((db(c.magnitude(SR, 1000.0)) + 3.01).abs() < 0.1);
        assert!(db(c.magnitude(SR, 10_000.0)) < -35.0);
    }

    #[test]
    fn highpass_response() {
        let c = Coeffs::highpass(SR, 100.0, std::f32::consts::FRAC_1_SQRT_2);
        assert!(db(c.magnitude(SR, 10.0)) < -35.0);
        assert!(db(c.magnitude(SR, 5000.0)).abs() < 0.1);
    }

    #[test]
    fn peak_and_shelves_hit_their_gains() {
        let p = Coeffs::peaking(SR, 800.0, 1.0, 6.0);
        assert!((db(p.magnitude(SR, 800.0)) - 6.0).abs() < 0.05);
        let ls = Coeffs::low_shelf(SR, 100.0, 0.707, -9.0);
        assert!((db(ls.magnitude(SR, 10.0)) + 9.0).abs() < 0.2);
        assert!(db(ls.magnitude(SR, 5000.0)).abs() < 0.2);
        let hs = Coeffs::high_shelf(SR, 3000.0, 0.707, 4.0);
        assert!((db(hs.magnitude(SR, 18_000.0)) - 4.0).abs() < 0.3);
    }

    #[test]
    fn filter_matches_analytic_response() {
        let c = Coeffs::peaking(SR, 1000.0, 1.0, 9.0);
        let mut f = Biquad::new(c);
        let freq = 1000.0;
        let mut peak: f32 = 0.0;
        for n in 0..48_000 {
            let x = (2.0 * PI * freq * n as f32 / SR).sin();
            let y = f.process(x);
            if n > 24_000 {
                peak = peak.max(y.abs());
            }
        }
        assert!((db(peak) - 9.0).abs() < 0.1, "{}", db(peak));
    }
}
