//! 4x polyphase FIR oversampler (linear phase, integer group delay).
//!
//! The up- and down-sampling filters are the same 49-tap Kaiser-windowed
//! low-pass at the 4x rate. The cascade's group delay is 48 high-rate samples,
//! exactly 12 base-rate samples, so the dry path can be delay-matched
//! without fractional error.

pub const FACTOR: usize = 4;
const TAPS: usize = 49;
/// Base-rate latency of an up+down round trip.
pub const LATENCY: usize = (TAPS - 1) / FACTOR;

pub struct Oversampler4x {
    h: [f32; TAPS],
    /// Base-rate input history for the polyphase upsampler (mirrored ring).
    up_hist: Vec<f32>,
    up_pos: usize,
    /// High-rate history for the decimator (mirrored ring).
    down_hist: Vec<f32>,
    down_pos: usize,
}

const UP_LEN: usize = TAPS.div_ceil(FACTOR);
/// The decimator evaluates FACTOR-1 high-rate samples in the past so the
/// kept phase lines up with the base-rate grid (integer total latency).
const DOWN_LEN: usize = TAPS + FACTOR - 1;

impl Default for Oversampler4x {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler4x {
    pub fn new() -> Self {
        Self {
            h: design(),
            up_hist: vec![0.0; UP_LEN * 2],
            up_pos: 0,
            down_hist: vec![0.0; DOWN_LEN * 2],
            down_pos: 0,
        }
    }

    pub fn reset(&mut self) {
        self.up_hist.fill(0.0);
        self.down_hist.fill(0.0);
        self.up_pos = 0;
        self.down_pos = 0;
    }

    /// One base-rate sample in, `FACTOR` high-rate samples out.
    #[inline]
    pub fn upsample(&mut self, x: f32, out: &mut [f32; FACTOR]) {
        self.up_pos = (self.up_pos + UP_LEN - 1) % UP_LEN;
        self.up_hist[self.up_pos] = x;
        self.up_hist[self.up_pos + UP_LEN] = x;
        let hist = &self.up_hist[self.up_pos..self.up_pos + UP_LEN];
        for (phase, o) in out.iter_mut().enumerate() {
            let mut acc = 0.0;
            let mut j = 0;
            let mut t = phase;
            while t < TAPS {
                acc += self.h[t] * hist[j];
                t += FACTOR;
                j += 1;
            }
            // Zero-stuffing loses a factor of FACTOR in level; restore it.
            *o = acc * FACTOR as f32;
        }
    }

    /// `FACTOR` high-rate samples in, one base-rate sample out.
    #[inline]
    pub fn downsample(&mut self, input: &[f32; FACTOR]) -> f32 {
        for &x in input {
            self.down_pos = (self.down_pos + DOWN_LEN - 1) % DOWN_LEN;
            self.down_hist[self.down_pos] = x;
            self.down_hist[self.down_pos + DOWN_LEN] = x;
        }
        let start = self.down_pos + FACTOR - 1;
        let hist = &self.down_hist[start..start + TAPS];
        let mut acc = 0.0;
        for (h, x) in self.h.iter().zip(hist) {
            acc += h * x;
        }
        acc
    }
}

fn design() -> [f32; TAPS] {
    // Pass-band edge ~0.45 of the base Nyquist, i.e. 0.1125 of the 4x rate.
    let cutoff = 0.45 / FACTOR as f64;
    let beta = 7.0;
    let m = (TAPS - 1) as f64;
    let mut h = [0.0f32; TAPS];
    let mut sum = 0.0;
    for (n, hn) in h.iter_mut().enumerate() {
        let t = n as f64 - m / 2.0;
        let sinc = if t == 0.0 {
            2.0 * cutoff
        } else {
            (2.0 * std::f64::consts::PI * cutoff * t).sin() / (std::f64::consts::PI * t)
        };
        let r = 2.0 * n as f64 / m - 1.0;
        let w = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta);
        *hn = (sinc * w) as f32;
        sum += sinc * w;
    }
    for hn in h.iter_mut() {
        *hn /= sum as f32;
    }
    h
}

pub(crate) fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let y = x * x / 4.0;
    for k in 1..50 {
        term *= y / (k * k) as f64;
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_roundtrip_has_integer_latency() {
        let mut os = Oversampler4x::new();
        let n = 4_000;
        let input: Vec<f32> = (0..n).map(|i| (i as f32 * 0.02).sin() * 0.5).collect();
        let mut out = vec![0.0; n];
        let mut hi = [0.0; FACTOR];
        for (i, &x) in input.iter().enumerate() {
            os.upsample(x, &mut hi);
            out[i] = os.downsample(&hi);
        }
        for i in 200..n {
            assert!(
                (out[i] - input[i - LATENCY]).abs() < 2e-3,
                "i={i} {} vs {}",
                out[i],
                input[i - LATENCY]
            );
        }
    }
}
