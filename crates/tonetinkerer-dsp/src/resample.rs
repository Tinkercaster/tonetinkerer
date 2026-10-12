//! Streaming windowed-sinc sample-rate converter.
//!
//! NAM models are only valid at the rate they were captured at (almost
//! always 48 kHz), so when the host runs at another rate the amp stage
//! converts host -> model rate -> host. Arbitrary ratio, per-sample push
//! interface, polyphase table with linear phase interpolation. All memory is
//! allocated in `new`; `push` is allocation-free.

use crate::oversample::bessel_i0;

pub struct SincResampler {
    step: f64,
    half: usize,
    phases: usize,
    table: Vec<f32>,
    hist: Vec<f32>,
    cap: usize,
    pos: usize,
    t_rel: f64,
}

impl SincResampler {
    pub fn new(in_rate: f64, out_rate: f64) -> Self {
        let cutoff = (out_rate / in_rate).min(1.0) * 0.91;
        let half = (16.0 / cutoff).ceil() as usize;
        let phases = 512;
        let taps = 2 * half;
        let beta = 8.5;
        let mut table = vec![0.0f32; (phases + 1) * taps];
        for p in 0..=phases {
            let frac = p as f64 / phases as f64;
            for j in 0..taps {
                // Tap j multiplies x[i0 - half + 1 + j]; its distance from the output time.
                let d = (j as f64 - half as f64 + 1.0) - frac;
                let sinc = if d.abs() < 1e-12 {
                    1.0
                } else {
                    let a = std::f64::consts::PI * cutoff * d;
                    a.sin() / a
                };
                let r = d / half as f64;
                let w = if r.abs() >= 1.0 {
                    0.0
                } else {
                    bessel_i0(beta * (1.0 - r * r).sqrt()) / bessel_i0(beta)
                };
                table[p * taps + j] = (cutoff * sinc * w) as f32;
            }
        }
        let cap = taps + 2;
        Self {
            step: in_rate / out_rate,
            half,
            phases,
            table,
            hist: vec![0.0; cap * 2],
            cap,
            pos: 0,
            t_rel: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.hist.fill(0.0);
        self.pos = 0;
        self.t_rel = 0.0;
    }

    /// Group delay in input samples (output k is input time k*step - half - 1).
    pub fn delay_in(&self) -> usize {
        self.half + 1
    }

    /// Upper bound on outputs produced for `n` inputs.
    pub fn max_out(&self, n: usize) -> usize {
        (n as f64 / self.step).ceil() as usize + 2
    }

    /// Push one input sample, calling `emit` for every output sample ready.
    #[inline]
    pub fn push(&mut self, x: f32, mut emit: impl FnMut(f32)) {
        // Newest sample lives at hist[pos] (and mirrored at pos + cap);
        // older samples at increasing indices.
        self.pos = (self.pos + self.cap - 1) % self.cap;
        self.hist[self.pos] = x;
        self.hist[self.pos + self.cap] = x;
        self.t_rel -= 1.0;
        let taps = 2 * self.half;
        while self.t_rel <= 0.0 {
            let fl = self.t_rel.floor();
            let frac = self.t_rel - fl;
            // i0 = newest - half + fl, where fl is 0 or -1.
            // Tap j uses x[i0 - half + 1 + j]; in "samples back from newest":
            // back = half - fl - (-half + 1 + j) = 2*half - 1 - fl - j.
            let back0 = (2 * self.half) as isize - 1 - fl as isize; // for j = 0
            let pf = frac * self.phases as f64;
            let p = (pf as usize).min(self.phases - 1);
            let a = (pf - p as f64) as f32;
            let t0 = &self.table[p * taps..(p + 1) * taps];
            let t1 = &self.table[(p + 1) * taps..(p + 2) * taps];
            // Samples j = 0..taps are at back0 - j: a contiguous reversed run.
            let start = self.pos as isize + back0 - (taps as isize - 1);
            let h = &self.hist[start as usize..start as usize + taps];
            let mut acc = 0.0f32;
            for j in 0..taps {
                let c = t0[j] + (t1[j] - t0[j]) * a;
                acc += c * h[taps - 1 - j];
            }
            emit(acc);
            self.t_rel += self.step;
        }
    }
}

/// Fixed-capacity single-threaded FIFO used to decouple the variable number
/// of resampled samples from the host's block size.
pub struct Fifo {
    buf: Vec<f32>,
    read: usize,
    len: usize,
}

impl Fifo {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0.0; capacity],
            read: 0,
            len: 0,
        }
    }
    pub fn clear(&mut self) {
        self.read = 0;
        self.len = 0;
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    #[inline]
    pub fn push(&mut self, x: f32) {
        if self.len == self.buf.len() {
            // Overflow can only happen if the drift bound is violated; drop the oldest.
            self.read = (self.read + 1) % self.buf.len();
            self.len -= 1;
        }
        let w = (self.read + self.len) % self.buf.len();
        self.buf[w] = x;
        self.len += 1;
    }
    #[inline]
    pub fn pop(&mut self) -> f32 {
        if self.len == 0 {
            return 0.0;
        }
        let x = self.buf[self.read];
        self.read = (self.read + 1) % self.buf.len();
        self.len -= 1;
        x
    }
}

/// Round-trip converter: host rate -> model rate, a model callback on
/// model-rate blocks, model rate -> host rate, with constant latency.
pub struct RoundTrip {
    up: SincResampler,
    down: SincResampler,
    scratch: Vec<f32>,
    fifo: Fifo,
    prefill: usize,
    latency: usize,
}

impl RoundTrip {
    pub fn new(host_rate: f64, model_rate: f64, max_block: usize) -> Self {
        let up = SincResampler::new(host_rate, model_rate);
        let down = SincResampler::new(model_rate, host_rate);
        let scratch = vec![0.0; up.max_out(max_block) + 4];
        let fifo = Fifo::new(max_block * 2 + 64);
        let mut rt = Self {
            up,
            down,
            scratch,
            fifo,
            prefill: 4,
            latency: 0,
        };
        rt.latency = rt.measure_latency(host_rate);
        rt.reset();
        rt
    }

    pub fn latency(&self) -> usize {
        self.latency
    }

    pub fn reset(&mut self) {
        self.up.reset();
        self.down.reset();
        self.fifo.clear();
        for _ in 0..self.prefill {
            self.fifo.push(0.0);
        }
    }

    /// Run a host-rate block through `model` at the model rate. Exactly
    /// `buf.len()` samples come back out; `buf.len()` must be <= `max_block`.
    pub fn process(&mut self, buf: &mut [f32], mut model: impl FnMut(&mut [f32])) {
        let mut m = 0;
        let scratch = &mut self.scratch;
        for &x in buf.iter() {
            self.up.push(x, |y| {
                if m < scratch.len() {
                    scratch[m] = y;
                    m += 1;
                }
            });
        }
        model(&mut scratch[..m]);
        for &y in scratch[..m].iter() {
            let fifo = &mut self.fifo;
            self.down.push(y, |z| fifo.push(z));
        }
        for x in buf.iter_mut() {
            *x = self.fifo.pop();
        }
    }

    /// Find the round-trip delay empirically from a band-limited click.
    fn measure_latency(&mut self, host_rate: f64) -> usize {
        self.reset();
        let n = 4096;
        let mut sig = vec![0.0f32; n];
        // Hann-windowed 1 kHz burst centred at sample 256: robust peak finding.
        let c = 256.0;
        for (i, s) in sig.iter_mut().enumerate() {
            let d = i as f64 - c;
            if d.abs() < 64.0 {
                let w = 0.5 + 0.5 * (std::f64::consts::PI * d / 64.0).cos();
                *s = (w * (2.0 * std::f64::consts::PI * 1000.0 * d / host_rate).cos()) as f32;
            }
        }
        let mut out = sig.clone();
        for chunk in out.chunks_mut(256) {
            self.process(chunk, |_| {});
        }
        let xc = |lag: usize| -> f64 {
            (0..n - lag)
                .map(|i| sig[i] as f64 * out[i + lag] as f64)
                .sum()
        };
        (0..1024)
            .max_by(|&a, &b| xc(a).partial_cmp(&xc(b)).unwrap())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(sr: f64, f: f64, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * f * i as f64 / sr).sin() as f32 * 0.5)
            .collect()
    }

    #[test]
    fn resampler_preserves_tone() {
        for (a, b) in [
            (44_100.0, 48_000.0),
            (96_000.0, 48_000.0),
            (48_000.0, 44_100.0),
        ] {
            let mut r = SincResampler::new(a, b);
            let x = sine(a, 1000.0, a as usize);
            let mut y = Vec::new();
            for &s in &x {
                r.push(s, |o| y.push(o));
            }
            let expected = (x.len() as f64 * b / a) as isize;
            assert!((y.len() as isize - expected).abs() <= 2);
            // Compare against an ideal sine at the output rate, skipping the warmup.
            let d = r.delay_in() as f64 / a;
            let mut err = 0.0f64;
            for (i, &v) in y.iter().enumerate().skip(2000).take(20_000) {
                let t = i as f64 / b - d;
                let ideal = 0.5 * (2.0 * std::f64::consts::PI * 1000.0 * t).sin();
                err = err.max((v as f64 - ideal).abs());
            }
            assert!(err < 2e-3, "{a}->{b}: err {err}");
        }
    }

    #[test]
    fn roundtrip_is_transparent_with_constant_latency() {
        for host in [44_100.0, 88_200.0, 96_000.0] {
            let mut rt = RoundTrip::new(host, 48_000.0, 512);
            let lat = rt.latency();
            assert!(lat > 0 && lat < 200, "latency {lat}");
            let x = sine(host, 440.0, host as usize);
            let mut y = x.clone();
            // Variable block sizes, as hosts do.
            let sizes = [1usize, 17, 512, 64, 333, 2, 128];
            let mut off = 0;
            let mut k = 0;
            while off < y.len() {
                let n = sizes[k % sizes.len()].min(y.len() - off);
                rt.process(&mut y[off..off + n], |_| {});
                off += n;
                k += 1;
            }
            let mut err = 0.0f32;
            for i in 4000..y.len() {
                err = err.max((y[i] - x[i - lat]).abs());
            }
            assert!(err < 0.02, "host {host}: err {err} lat {lat}");
        }
    }
}
