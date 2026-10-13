//! Cabinet impulse-response convolution.
//!
//! IRs are loaded from WAV (any rate, int or float, first channel), resampled
//! to the host rate, truncated, and level-normalized so the loudest point of
//! their 100 Hz - 6 kHz response sits at 0 dB. Partitioned FFT convolution is
//! done by `fft-convolver` (zero latency, allocation-free after `init`).

use crate::biquad::{Biquad, Coeffs};
use crate::resample::SincResampler;
use crate::util::{Fade, Smoothed};
use fft_convolver::FFTConvolver;
use realfft::RealFftPlanner;

pub const SLOT_4X12: usize = 0;
pub const SLOT_2X12: usize = 1;
pub const SLOT_1X12: usize = 2;
pub const SLOT_4X12_MODERN: usize = 3;
pub const SLOT_4X12_BRITISH: usize = 4;
pub const SLOT_CUSTOM: usize = 5;
pub const NUM_SLOTS: usize = 6;

/// Longest IR kept (cabinet IRs rarely need more than ~100 ms).
pub const MAX_IR_SECONDS: f32 = 0.5;
const PARTITION: usize = 128;

#[derive(Debug)]
pub struct IrError(pub String);

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IrError {}

/// Decode a WAV file from any reader into (first-channel samples, sample rate).
pub fn decode_wav<R: std::io::Read>(reader: R) -> Result<(Vec<f32>, f32), IrError> {
    let mut r = hound::WavReader::new(reader).map_err(|e| IrError(e.to_string()))?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let data: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| IrError(e.to_string()))?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample.saturating_sub(1))) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(|e| IrError(e.to_string()))?
        }
    };
    let mono: Vec<f32> = data.chunks(ch).map(|c| c[0]).collect();
    if mono.is_empty() {
        return Err(IrError("IR file contains no samples".into()));
    }
    if mono.iter().any(|x| !x.is_finite()) {
        return Err(IrError("IR file contains non-finite samples".into()));
    }
    Ok((mono, spec.sample_rate as f32))
}

/// Resample, truncate and normalize an IR for use at `host_rate`.
pub fn prepare_ir(ir: &[f32], ir_rate: f32, host_rate: f32) -> Result<Vec<f32>, IrError> {
    let mut out: Vec<f32> = if (ir_rate - host_rate).abs() < 0.5 {
        ir.to_vec()
    } else {
        let mut r = SincResampler::new(ir_rate as f64, host_rate as f64);
        let delay = r.delay_in() as f64 * host_rate as f64 / ir_rate as f64;
        let mut v =
            Vec::with_capacity((ir.len() as f64 * host_rate as f64 / ir_rate as f64) as usize + 64);
        for &x in ir.iter().chain(std::iter::repeat_n(&0.0, r.delay_in() + 4)) {
            r.push(x, |y| v.push(y));
        }
        // Drop the converter's group delay so the IR's onset stays at t = 0.
        let d = delay.round() as usize;
        v.drain(..d.min(v.len()));
        v.iter_mut().for_each(|x| *x *= ir_rate / host_rate);
        v
    };
    let max_len = (MAX_IR_SECONDS * host_rate) as usize;
    if out.len() > max_len {
        out.truncate(max_len);
        // Short fade so truncation never clicks.
        let fade = (0.005 * host_rate) as usize;
        let n = out.len();
        for i in 0..fade.min(n) {
            out[n - 1 - i] *= i as f32 / fade as f32;
        }
    }
    let peak = peak_response(&out, host_rate);
    if !(peak > 1e-9) {
        return Err(IrError("IR is silent".into()));
    }
    out.iter_mut().for_each(|x| *x /= peak);
    Ok(out)
}

/// Maximum magnitude of the IR's frequency response between 100 Hz and 6 kHz.
pub fn peak_response(ir: &[f32], sr: f32) -> f32 {
    let n = (ir.len().max(4096)).next_power_of_two() * 2;
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    let mut input = vec![0.0f32; n];
    input[..ir.len()].copy_from_slice(ir);
    let mut spec = fft.make_output_vec();
    fft.process(&mut input, &mut spec).ok();
    let lo = (100.0 * n as f32 / sr) as usize;
    let hi = ((6_000.0 * n as f32 / sr) as usize).min(spec.len() - 1);
    spec[lo..=hi].iter().map(|c| c.norm()).fold(0.0, f32::max)
}

pub struct CabSlot {
    conv: FFTConvolver<f32>,
    pub name: String,
}

impl CabSlot {
    /// Build from an already prepared IR (allocates).
    pub fn new(prepared_ir: &[f32], name: &str) -> Result<Self, IrError> {
        let mut conv = FFTConvolver::default();
        conv.init(PARTITION, prepared_ir)
            .map_err(|e| IrError(format!("{e:?}")))?;
        Ok(Self {
            conv,
            name: name.to_owned(),
        })
    }

    pub fn from_wav_bytes(bytes: &[u8], name: &str, host_rate: f32) -> Result<Self, IrError> {
        let (ir, rate) = decode_wav(std::io::Cursor::new(bytes))?;
        Self::new(&prepare_ir(&ir, rate, host_rate)?, name)
    }

    pub fn from_file(path: &std::path::Path, host_rate: f32) -> Result<Self, IrError> {
        let f =
            std::fs::File::open(path).map_err(|e| IrError(format!("{}: {e}", path.display())))?;
        let (ir, rate) = decode_wav(std::io::BufReader::new(f))?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::new(&prepare_ir(&ir, rate, host_rate)?, &name)
    }

    pub fn reset(&mut self) {
        self.conv.reset();
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        if self.conv.process(input, output).is_err() {
            output.fill(0.0);
        }
    }
}

/// 24 dB/oct Butterworth low/high cuts (two biquad sections each); state is kept.
fn design_cuts(sr: f32, lc: f32, hc: f32, low: &mut [Biquad; 2], high: &mut [Biquad; 2]) {
    let (q1, q2) = (0.541_196_1, 1.306_563);
    low[0].c = Coeffs::highpass(sr, lc, q1);
    low[1].c = Coeffs::highpass(sr, lc, q2);
    high[0].c = Coeffs::lowpass(sr, hc, q1);
    high[1].c = Coeffs::lowpass(sr, hc, q2);
}

pub struct CabStage {
    sr: f32,
    slots: [Option<Box<CabSlot>>; NUM_SLOTS],
    active: usize,
    prev: Option<usize>,
    fade_pos: usize,
    fade_len: usize,
    enabled: Fade,
    low_cut: [Biquad; 2],
    high_cut: [Biquad; 2],
    lc: Smoothed,
    hc: Smoothed,
    counter: u32,
    dry: Vec<f32>,
    wet: Vec<f32>,
    old: Vec<f32>,
}

impl CabStage {
    pub fn new(sr: f32, max_block: usize) -> Self {
        let mut enabled = Fade::new(true);
        enabled.set_time(sr, 15.0);
        let mut lc = Smoothed::new(80.0);
        let mut hc = Smoothed::new(8_000.0);
        lc.set_time(sr, 25.0);
        hc.set_time(sr, 25.0);
        let mut s = Self {
            sr,
            slots: [None, None, None, None, None, None],
            active: SLOT_4X12,
            prev: None,
            fade_pos: 0,
            fade_len: (sr * 0.03) as usize,
            enabled,
            low_cut: [Biquad::default(); 2],
            high_cut: [Biquad::default(); 2],
            lc,
            hc,
            counter: 0,
            dry: vec![0.0; max_block],
            wet: vec![0.0; max_block],
            old: vec![0.0; max_block],
        };
        s.update_filters();
        s
    }

    pub fn set_slot(&mut self, idx: usize, slot: Option<Box<CabSlot>>) -> Option<Box<CabSlot>> {
        std::mem::replace(&mut self.slots[idx], slot)
    }

    pub fn slot_name(&self, idx: usize) -> Option<&str> {
        self.slots[idx].as_deref().map(|s| s.name.as_str())
    }

    /// Hand over a custom IR. Returns the replaced slot for off-thread dropping.
    /// IR swaps on the active slot are crossfaded from the (silent) restarted
    /// convolver, so they never click.
    pub fn swap_custom(&mut self, mut incoming: Box<CabSlot>) -> Option<Box<CabSlot>> {
        incoming.reset();
        let old = self.slots[SLOT_CUSTOM].replace(incoming);
        if self.active == SLOT_CUSTOM {
            self.fade_pos = 0;
        }
        old
    }

    fn resolve(&self, wanted: usize) -> usize {
        if self.slots[wanted].is_some() {
            wanted
        } else {
            SLOT_4X12
        }
    }

    fn update_filters(&mut self) {
        design_cuts(
            self.sr,
            self.lc.value(),
            self.hc.value(),
            &mut self.low_cut,
            &mut self.high_cut,
        );
    }

    pub fn reset(&mut self, enabled: bool, wanted: usize, low_cut: f32, high_cut: f32) {
        for s in self.slots.iter_mut().flatten() {
            s.reset();
        }
        self.active = self.resolve(wanted);
        self.prev = None;
        self.enabled.reset(enabled);
        self.lc.reset(low_cut);
        self.hc.reset(high_cut);
        for f in self.low_cut.iter_mut().chain(self.high_cut.iter_mut()) {
            f.reset();
        }
        self.update_filters();
    }

    pub fn process(
        &mut self,
        buf: &mut [f32],
        enabled: bool,
        wanted: usize,
        low_cut: f32,
        high_cut: f32,
    ) {
        let n = buf.len();
        self.enabled.set_engaged(enabled);
        self.lc.set_target(low_cut);
        self.hc.set_target(high_cut);

        let target = self.resolve(wanted);
        if target != self.active && self.prev.is_none() {
            if let Some(s) = self.slots[target].as_deref_mut() {
                s.reset();
            }
            self.prev = Some(self.active);
            self.active = target;
            self.fade_pos = 0;
        }

        if self.enabled.is_off() {
            return;
        }
        self.dry[..n].copy_from_slice(buf);
        let (dry, wet, old) = (&self.dry[..n], &mut self.wet[..n], &mut self.old[..n]);
        let slots = &mut self.slots;
        match slots[self.active].as_deref_mut() {
            Some(s) => s.process(dry, wet),
            None => wet.copy_from_slice(dry),
        }
        let fading = self.fade_pos < self.fade_len;
        if fading {
            match self.prev.and_then(|p| slots[p].as_deref_mut()) {
                Some(s) => s.process(dry, old),
                None => old.fill(0.0),
            }
            for (w, o) in wet.iter_mut().zip(old.iter()) {
                let t = (self.fade_pos as f32 / self.fade_len as f32).min(1.0);
                *w = *w * t + *o * (1.0 - t);
                self.fade_pos += 1;
            }
            if self.fade_pos >= self.fade_len {
                self.prev = None;
            }
        }

        for i in 0..n {
            self.lc.next();
            self.hc.next();
            self.counter += 1;
            if self.counter >= 16 {
                self.counter = 0;
                if !self.lc.is_settled() || !self.hc.is_settled() {
                    design_cuts(
                        self.sr,
                        self.lc.value(),
                        self.hc.value(),
                        &mut self.low_cut,
                        &mut self.high_cut,
                    );
                }
            }
            let mut y = wet[i];
            for f in self.low_cut.iter_mut().chain(self.high_cut.iter_mut()) {
                y = f.process(y);
            }
            let e = self.enabled.next();
            buf[i] = dry[i] + (y - dry[i]) * e;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ir_passes_signal() {
        let mut ir = vec![0.0f32; 64];
        ir[0] = 1.0;
        let prepared = prepare_ir(&ir, 48_000.0, 48_000.0).unwrap();
        assert!((prepared[0] - 1.0).abs() < 1e-3);
        let mut stage = CabStage::new(48_000.0, 512);
        stage.set_slot(
            SLOT_4X12,
            Some(Box::new(CabSlot::new(&prepared, "id").unwrap())),
        );
        stage.reset(true, SLOT_4X12, 20.0, 20_000.0);
        let x: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.03).sin() * 0.5).collect();
        let mut y = x.clone();
        for c in y.chunks_mut(100) {
            stage.process(c, true, SLOT_4X12, 20.0, 20_000.0);
        }
        // The 20 Hz / 20 kHz cuts shift phase slightly, so compare level.
        let rms = |v: &[f32]| (v.iter().map(|a| a * a).sum::<f32>() / v.len() as f32).sqrt();
        let ratio_db = 20.0 * (rms(&y[2000..]) / rms(&x[2000..])).log10();
        assert!(ratio_db.abs() < 0.3, "level changed by {ratio_db} dB");
    }

    #[test]
    fn resampled_ir_keeps_onset_and_level() {
        let mut ir = vec![0.0f32; 2000];
        ir[0] = 1.0;
        let p = prepare_ir(&ir, 44_100.0, 96_000.0).unwrap();
        let (imax, _) = p.iter().enumerate().fold((0, 0.0f32), |(bi, bv), (i, &v)| {
            if v.abs() > bv { (i, v.abs()) } else { (bi, bv) }
        });
        assert!(imax <= 2, "onset moved to {imax}");
        assert!((peak_response(&p, 96_000.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn rejects_silent_ir() {
        assert!(prepare_ir(&[0.0; 100], 48_000.0, 48_000.0).is_err());
    }
}
