//! Stereo algorithmic reverb: pre-delay -> input diffusion (series
//! all-passes) -> 8-line feedback delay network with a Householder mixing
//! matrix, per-line high-frequency damping and slow delay modulation to keep
//! long tails lush rather than metallic. Decay time is specified as RT60.

use crate::util::{Smoothed, undenormal};
use std::f32::consts::PI;

const N: usize = 8;
/// Base line lengths in ms (mutually prime-ish at common rates).
const LINE_MS: [f32; N] = [37.1, 41.3, 47.9, 53.3, 59.9, 67.7, 73.1, 79.3];
const DIFF_MS: [f32; 4] = [4.7, 3.6, 12.7, 9.3];
const MAX_PREDELAY_MS: f32 = 250.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbSettings {
    pub decay_s: f32,
    pub predelay_ms: f32,
    /// 0 = bright, 1 = very dark tail.
    pub damping: f32,
    pub mix: f32,
}

impl Default for ReverbSettings {
    fn default() -> Self {
        Self {
            decay_s: 2.5,
            predelay_ms: 20.0,
            damping: 0.4,
            mix: 0.2,
        }
    }
}

struct Allpass {
    buf: Vec<f32>,
    pos: usize,
    g: f32,
}

impl Allpass {
    fn new(len: usize, g: f32) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            pos: 0,
            g,
        }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let d = self.buf[self.pos];
        let v = x + d * self.g;
        self.buf[self.pos] = undenormal(v);
        self.pos = (self.pos + 1) % self.buf.len();
        d - v * self.g
    }
    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }
}

pub struct Reverb {
    sr: f32,
    pre: Vec<f32>,
    pre_pos: usize,
    diff: [Allpass; 4],
    lines: [Vec<f32>; N],
    pos: [usize; N],
    len: [f32; N],
    damp_state: [f32; N],
    lfo: f32,
    decay: Smoothed,
    mix: Smoothed,
    predelay: Smoothed,
    gains: [f32; N],
    damp_c: f32,
    counter: u32,
}

impl Reverb {
    pub fn new(sr: f32) -> Self {
        let lines: [Vec<f32>; N] =
            std::array::from_fn(|i| vec![0.0; (LINE_MS[i] * 0.001 * sr) as usize + 32]);
        let len: [f32; N] = std::array::from_fn(|i| LINE_MS[i] * 0.001 * sr);
        let diff = std::array::from_fn(|i| Allpass::new((DIFF_MS[i] * 0.001 * sr) as usize, 0.62));
        let mut decay = Smoothed::new(2.5);
        decay.set_time(sr, 50.0);
        let mut mix = Smoothed::new(0.2);
        mix.set_time(sr, 30.0);
        let mut predelay = Smoothed::new(20.0);
        predelay.set_time(sr, 80.0);
        let mut r = Self {
            sr,
            pre: vec![0.0; (MAX_PREDELAY_MS * 0.001 * sr) as usize + 4],
            pre_pos: 0,
            diff,
            lines,
            pos: [0; N],
            len,
            damp_state: [0.0; N],
            lfo: 0.0,
            decay,
            mix,
            predelay,
            gains: [0.0; N],
            damp_c: 0.5,
            counter: 0,
        };
        r.update(0.4);
        r
    }

    fn update(&mut self, damping: f32) {
        let rt = self.decay.value().max(0.1);
        for i in 0..N {
            // Gain giving -60 dB after `rt` seconds for this line's length.
            self.gains[i] = 10f32.powf(-3.0 * self.len[i] / (rt * self.sr));
        }
        let cutoff = 16_000.0 * (1_500.0f32 / 16_000.0).powf(damping.clamp(0.0, 1.0));
        self.damp_c = 1.0 - (-2.0 * PI * cutoff / self.sr).exp();
    }

    pub fn reset(&mut self, s: &ReverbSettings) {
        self.pre.fill(0.0);
        for d in self.diff.iter_mut() {
            d.reset();
        }
        for l in self.lines.iter_mut() {
            l.fill(0.0);
        }
        self.damp_state = [0.0; N];
        // Restart the modulation too: a reset must sound exactly like a
        // freshly built engine (hosts reset instead of re-creating before a render).
        self.lfo = 0.0;
        self.decay.reset(s.decay_s.clamp(0.2, 20.0));
        self.mix.reset(s.mix);
        self.predelay
            .reset(s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS - 1.0));
        self.update(s.damping);
    }

    #[inline]
    fn read_line(&self, i: usize, delay: f32) -> f32 {
        let buf = &self.lines[i];
        let l = buf.len();
        let d = delay.clamp(1.0, (l - 2) as f32);
        let di = d as usize;
        let f = d - di as f32;
        let a = buf[(self.pos[i] + l - di) % l];
        let b = buf[(self.pos[i] + l - di - 1) % l];
        a + (b - a) * f
    }

    /// Add the reverb of the stereo feed (`feed_l`/`feed_r`, already scaled by
    /// the bypass fade so tails ring out) onto `left`/`right`.
    /// Returns the peak wet level of this block (for tail detection).
    pub fn process(
        &mut self,
        feed_l: &[f32],
        feed_r: &[f32],
        left: &mut [f32],
        right: &mut [f32],
        s: &ReverbSettings,
    ) -> f32 {
        self.decay.set_target(s.decay_s.clamp(0.2, 20.0));
        self.mix.set_target(s.mix.clamp(0.0, 1.0));
        self.predelay
            .set_target(s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS - 1.0));
        let lfo_inc = 2.0 * PI * 0.27 / self.sr;
        let mod_depth = 0.0009 * self.sr;
        let mut peak = 0.0f32;
        for i in 0..left.len() {
            self.decay.next();
            self.counter += 1;
            if self.counter >= 64 {
                self.counter = 0;
                self.update(s.damping);
            }
            let mix = self.mix.next();
            let input = 0.5 * (feed_l[i] + feed_r[i]);

            // Pre-delay.
            let pl = self.pre.len();
            self.pre[self.pre_pos] = input;
            let pd = (self.predelay.next() * 0.001 * self.sr) as usize;
            let mut x = self.pre[(self.pre_pos + pl - pd.min(pl - 1)) % pl];
            self.pre_pos = (self.pre_pos + 1) % pl;

            for d in self.diff.iter_mut() {
                x = d.process(x);
            }

            self.lfo += lfo_inc;
            if self.lfo > 2.0 * PI {
                self.lfo -= 2.0 * PI;
            }
            let mut outs = [0.0f32; N];
            for (k, o) in outs.iter_mut().enumerate() {
                let m = if k % 2 == 0 {
                    (self.lfo + k as f32).sin() * mod_depth
                } else {
                    0.0
                };
                let y = self.read_line(k, self.len[k] + m);
                self.damp_state[k] =
                    undenormal(self.damp_state[k] + (y - self.damp_state[k]) * self.damp_c);
                *o = self.damp_state[k] * self.gains[k];
            }
            // Householder reflection: y = x - (2/N) * sum(x).
            let sum: f32 = outs.iter().sum::<f32>() * (2.0 / N as f32);
            for k in 0..N {
                let fb = outs[k] - sum;
                let inj = if k % 2 == 0 { x } else { -x };
                let l = self.lines[k].len();
                self.lines[k][self.pos[k]] = fb + inj * 0.35;
                self.pos[k] = (self.pos[k] + 1) % l;
            }
            let wl = outs[0] - outs[2] + outs[4] - outs[6] + 0.5 * (outs[1] - outs[5]);
            let wr = outs[1] - outs[3] + outs[5] - outs[7] + 0.5 * (outs[2] - outs[6]);
            let g = mix * 0.6;
            left[i] += wl * g;
            right[i] += wr * g;
            peak = peak.max((wl * g).abs()).max((wr * g).abs());
            if !left[i].is_finite() || !right[i].is_finite() {
                self.reset(s);
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

    fn energy(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum()
    }

    #[test]
    fn tail_decays_at_roughly_rt60() {
        let sr = 48_000.0;
        let mut r = Reverb::new(sr);
        let s = ReverbSettings {
            decay_s: 1.0,
            predelay_ms: 0.0,
            damping: 0.0,
            mix: 1.0,
        };
        r.reset(&s);
        let n = (sr * 2.5) as usize;
        let mut l = vec![0.0; n];
        let mut rr = vec![0.0; n];
        l[0] = 1.0;
        rr[0] = 1.0;
        let mut dl = vec![0.0; n];
        let mut dr = vec![0.0; n];
        r.process(&l, &rr, &mut dl, &mut dr, &s);
        let w = (0.1 * sr) as usize;
        let e1 = energy(&dl[(0.2 * sr) as usize..(0.2 * sr) as usize + w]);
        let e2 = energy(&dl[(0.7 * sr) as usize..(0.7 * sr) as usize + w]);
        // 0.5 s apart at RT60 = 1 s: about 30 dB of decay (allow a broad window).
        let drop = 10.0 * (e1 / e2).log10();
        assert!(drop > 20.0 && drop < 40.0, "decay over 0.5 s: {drop} dB");
        // Decorrelated stereo output.
        assert!(energy(&dr) > 0.0);
        assert!(dl.iter().zip(&dr).any(|(a, b)| (a - b).abs() > 1e-3));
    }

    #[test]
    fn long_decay_is_stable() {
        let sr = 48_000.0;
        let mut r = Reverb::new(sr);
        let s = ReverbSettings {
            decay_s: 20.0,
            predelay_ms: 50.0,
            damping: 0.0,
            mix: 1.0,
        };
        r.reset(&s);
        let n = (sr * 10.0) as usize;
        let mut l: Vec<f32> = (0..n)
            .map(|i| {
                if i < 4800 {
                    ((i as f32) * 0.3).sin()
                } else {
                    0.0
                }
            })
            .collect();
        let feed = l.clone();
        let mut rr = l.clone();
        r.process(&feed, &feed, &mut l, &mut rr, &s);
        assert!(l.iter().all(|x| x.is_finite() && x.abs() < 4.0));
    }
}
