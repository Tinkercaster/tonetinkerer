//! Synthesized guitar-cabinet impulse responses for the bundled IRs.
//!
//! Each cab is specified as a magnitude curve (dB at anchor frequencies,
//! interpolated in log-frequency) describing a close-miked speaker: low-end
//! cabinet resonance, upper-mid cone break-up peaks and the steep
//! high-frequency roll-off that makes a guitar speaker sound like one.
//! Deterministic fine ripple adds cone irregularity; the response is made
//! minimum-phase (real speakers are essentially minimum-phase) via the real
//! cepstrum, then cabinet / room reflections are added in the time domain.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

pub struct CabSpec {
    pub name: &'static str,
    pub points: &'static [(f32, f32)],
    /// (delay ms, gain) reflections, negative gain = polarity inverted.
    pub reflections: &'static [(f32, f32)],
    pub ripple_db: f32,
    pub seed: u32,
}

pub const CABS: [CabSpec; 3] = [
    CabSpec {
        // Closed-back 4x12 with British-voiced 12" speakers: tight lows,
        // aggressive 2-4 kHz bite, firm roll-off above 5 kHz.
        name: "4x12_closed",
        points: &[
            (20.0, -32.0),
            (50.0, -12.0),
            (80.0, -3.0),
            (110.0, 2.0),
            (160.0, 0.0),
            (250.0, -1.0),
            (400.0, -2.5),
            (700.0, -0.5),
            (1_200.0, 1.0),
            (2_000.0, 3.0),
            (2_800.0, 5.0),
            (3_500.0, 2.0),
            (4_200.0, 3.5),
            (5_000.0, 0.0),
            (6_000.0, -4.0),
            (7_000.0, -8.0),
            (8_000.0, -12.0),
            (10_000.0, -20.0),
            (14_000.0, -34.0),
            (24_000.0, -55.0),
        ],
        reflections: &[(1.3, 0.14), (2.9, -0.07), (6.1, 0.04)],
        ripple_db: 1.5,
        seed: 1,
    },
    CabSpec {
        // Open-back 2x12: looser, wider low end, chimey and slightly scooped.
        name: "2x12_open",
        points: &[
            (20.0, -36.0),
            (60.0, -12.0),
            (90.0, -4.0),
            (120.0, 0.0),
            (200.0, 1.0),
            (400.0, 0.0),
            (800.0, -1.5),
            (1_500.0, 1.0),
            (2_500.0, 3.0),
            (3_200.0, 1.0),
            (4_000.0, 2.5),
            (5_000.0, 0.0),
            (6_500.0, -6.0),
            (8_000.0, -12.0),
            (10_000.0, -20.0),
            (15_000.0, -34.0),
            (24_000.0, -55.0),
        ],
        reflections: &[(0.9, -0.22), (3.8, 0.09), (9.5, 0.05)],
        ripple_db: 1.2,
        seed: 2,
    },
    CabSpec {
        // Vintage 1x12 combo: warm, rounded, gentle top - lovely for cleans.
        name: "1x12_vintage",
        points: &[
            (20.0, -30.0),
            (70.0, -10.0),
            (100.0, -2.0),
            (150.0, 1.0),
            (300.0, 1.0),
            (600.0, 0.0),
            (1_000.0, -1.0),
            (1_800.0, 1.0),
            (2_500.0, 2.0),
            (3_200.0, -1.0),
            (4_000.0, -3.0),
            (5_500.0, -8.0),
            (7_000.0, -14.0),
            (10_000.0, -25.0),
            (24_000.0, -55.0),
        ],
        reflections: &[(0.7, -0.18), (2.2, 0.08)],
        ripple_db: 1.0,
        seed: 3,
    },
];

fn interp_db(points: &[(f32, f32)], f: f32) -> f32 {
    if f <= points[0].0 {
        return points[0].1;
    }
    for w in points.windows(2) {
        let (f0, d0) = w[0];
        let (f1, d1) = w[1];
        if f <= f1 {
            let t = (f.ln() - f0.ln()) / (f1.ln() - f0.ln());
            // Smoothstep in log-frequency avoids kinks at the anchors.
            let t = t * t * (3.0 - 2.0 * t);
            return d0 + (d1 - d0) * t;
        }
    }
    points.last().unwrap().1
}

pub fn generate(spec: &CabSpec, sr: f32) -> Vec<f32> {
    let n = 16_384usize;
    let mut planner = FftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(n);
    let inv = planner.plan_fft_inverse(n);

    // Deterministic ripple: a few incommensurate sinusoids in log-frequency.
    let s = spec.seed as f32;
    let ripple = |f: f32| -> f32 {
        let x = f.max(20.0).log2();
        spec.ripple_db
            * (0.5 * (x * 7.3 + s).sin()
                + 0.3 * (x * 13.1 + 2.0 * s).sin()
                + 0.2 * (x * 23.7 + 3.0 * s).sin())
            * ((f - 300.0) / 2_000.0).clamp(0.0, 1.0)
    };

    // Log magnitude spectrum (even symmetric).
    let mut buf: Vec<Complex<f64>> = (0..n)
        .map(|k| {
            let kk = if k <= n / 2 { k } else { n - k };
            let f = (kk as f32 * sr / n as f32).max(1.0);
            let db = interp_db(spec.points, f) + ripple(f);
            Complex::new((db as f64 / 20.0) * std::f64::consts::LN_10, 0.0)
        })
        .collect();

    // Real cepstrum -> fold to causal -> minimum-phase spectrum.
    inv.process(&mut buf);
    for c in buf.iter_mut() {
        *c /= n as f64;
    }
    for k in 1..n / 2 {
        buf[k] *= 2.0;
    }
    for c in buf.iter_mut().skip(n / 2 + 1) {
        *c = Complex::new(0.0, 0.0);
    }
    fwd.process(&mut buf);
    for c in buf.iter_mut() {
        *c = c.exp();
    }
    inv.process(&mut buf);
    let mut h: Vec<f32> = buf.iter().map(|c| (c.re / n as f64) as f32).collect();

    // Keep 120 ms with a smooth tail fade.
    let keep = (0.12 * sr) as usize;
    h.truncate(keep);
    let fade = (0.03 * sr) as usize;
    for i in 0..fade {
        let k = keep - fade + i;
        h[k] *= 0.5 + 0.5 * (std::f32::consts::PI * i as f32 / fade as f32).cos();
    }

    // Reflections (each slightly low-passed: absorption).
    let direct = h.clone();
    for &(ms, g) in spec.reflections {
        let d = (ms * 0.001 * sr) as usize;
        let mut lp = 0.0;
        for i in 0..keep.saturating_sub(d) {
            lp += (direct[i] - lp) * 0.35;
            h[i + d] += g * lp;
        }
    }

    let peak = h.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    h.iter_mut().for_each(|v| *v *= 0.9 / peak);
    h
}
