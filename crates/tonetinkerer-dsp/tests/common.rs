#![allow(dead_code)]
//! Shared helpers for the offline-rendering integration tests.

use tonetinkerer_dsp::{ChainParams, Engine};

pub fn pluck_phrase(sr: f32, seconds: f32) -> Vec<f32> {
    // Simple deterministic plucked notes (decaying harmonic tones).
    let n = (sr * seconds) as usize;
    let mut out = vec![0.0f32; n];
    let notes = [
        (0.1, 330.0),
        (0.6, 392.0),
        (1.1, 440.0),
        (1.6, 494.0),
        (2.2, 659.0),
    ];
    for &(t, f) in notes.iter() {
        let start = (t * sr) as usize;
        for i in start..n {
            let tt = (i - start) as f32 / sr;
            let env = (-tt * 2.5).exp() * (tt * 400.0).min(1.0);
            let mut v = 0.0;
            for h in 1..8 {
                v += (2.0 * std::f32::consts::PI * f * h as f32 * tt).sin() / (h * h) as f32;
            }
            out[i] += 0.3 * env * v;
        }
    }
    out
}

pub fn render(
    engine: &mut Engine,
    p: &ChainParams,
    input: &[f32],
    block: usize,
) -> (Vec<f32>, Vec<f32>) {
    let mut l = input.to_vec();
    let mut r = input.to_vec();
    for (cl, cr) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        engine.process(cl, Some(cr), p);
    }
    (l, r)
}

pub fn peak(v: &[f32]) -> f32 {
    v.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

pub fn rms(v: &[f32]) -> f32 {
    (v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt() as f32
}

pub fn all_finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// Simple deterministic LCG for randomized block sizes / automation.
pub struct Lcg(pub u64);
impl Lcg {
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn unit(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
}
