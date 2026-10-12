//! The complete signal chain and its plain-data parameter set.
//!
//! guitar in -> input select/trim -> noise gate -> wah -> sustain compressor
//! -> overdrive -> NAM amp -> cabinet IR -> EQ -> stereo delay -> reverb
//! -> output level -> safety limiter -> out
//!
//! `Engine::process` accepts any host block size (it works internally in
//! chunks of at most `MAX_CHUNK` samples), never allocates, locks or does I/O.

use crate::amp::{self, AmpStage, NamSlot};
use crate::cab::{self, CabSlot, CabStage};
use crate::chorus::{Chorus, ChorusSettings};
use crate::compressor::SustainCompressor;
use crate::delay::{DelaySettings, StereoDelay};
use crate::eq::{Eq, EqSettings};
use crate::gate::NoiseGate;
use crate::overdrive::Overdrive;
use crate::oversample;
use crate::reverb::{Reverb, ReverbSettings};
use crate::util::{Fade, Smoothed, db_to_gain, sanitize};
use crate::wah::{Wah, WahMode};

pub const MAX_CHUNK: usize = 256;

/// Fixed make-up after the cab/EQ: normalized amp (-18 dBFS) plus a
/// peak-normalized IR leaves typical playing around -22 dBFS RMS; this
/// brings it to a healthier ~-18 dBFS while keeping headroom for effects.
const MAKEUP_DB: f32 = 4.0;

/// Which input channel carries the guitar. Audio interfaces put the
/// instrument input on channel 1 or 2; hosts often duplicate a mono input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Follow whichever channel has the guitar (or average identical ones).
    #[default]
    Auto,
    Left,
    Right,
}

/// The kind of pickups on the guitar. Sounds are designed for humbuckers;
/// single coils are quieter and active pickups much hotter, so the input is
/// offset to make every sound respond as designed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GuitarType {
    SingleCoil,
    #[default]
    Humbucker,
    Active,
}

impl GuitarType {
    pub fn input_offset_db(self) -> f32 {
        match self {
            GuitarType::SingleCoil => 4.0,
            GuitarType::Humbucker => 0.0,
            GuitarType::Active => -5.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChainParams {
    pub input_mode: InputMode,
    pub guitar: GuitarType,
    pub input_gain_db: f32,
    pub gate_enabled: bool,
    pub gate_threshold_db: f32,
    pub wah_mode: WahMode,
    pub wah_position: f32,
    pub wah_sensitivity: f32,
    pub comp_enabled: bool,
    pub sustain: f32,
    pub drive_enabled: bool,
    pub drive: f32,
    pub drive_tone: f32,
    pub drive_level_db: f32,
    pub drive_fat: bool,
    pub amp_model: usize,
    pub amp_gain_db: f32,
    pub amp_level_db: f32,
    pub cab_enabled: bool,
    pub cab_ir: usize,
    pub cab_low_cut: f32,
    pub cab_high_cut: f32,
    pub eq: EqSettings,
    pub chorus_enabled: bool,
    pub chorus: ChorusSettings,
    pub delay_enabled: bool,
    pub delay: DelaySettings,
    pub reverb_enabled: bool,
    pub reverb: ReverbSettings,
    pub output_db: f32,
    pub limiter: bool,
}

impl Default for ChainParams {
    fn default() -> Self {
        Self {
            input_mode: InputMode::Auto,
            guitar: GuitarType::Humbucker,
            input_gain_db: 0.0,
            gate_enabled: true,
            gate_threshold_db: -62.0,
            wah_mode: WahMode::Off,
            wah_position: 0.5,
            wah_sensitivity: 0.5,
            comp_enabled: true,
            sustain: 0.4,
            drive_enabled: true,
            drive: 0.35,
            drive_tone: 0.55,
            drive_level_db: 3.0,
            drive_fat: false,
            amp_model: amp::SLOT_LEAD,
            amp_gain_db: 0.0,
            amp_level_db: 0.0,
            cab_enabled: true,
            cab_ir: cab::SLOT_4X12,
            cab_low_cut: 80.0,
            cab_high_cut: 8_000.0,
            eq: EqSettings::default(),
            chorus_enabled: false,
            chorus: ChorusSettings::default(),
            delay_enabled: true,
            delay: DelaySettings::default(),
            reverb_enabled: true,
            reverb: ReverbSettings::default(),
            output_db: 0.0,
            limiter: true,
        }
    }
}

impl ChainParams {
    /// Every tone-relevant value as a flat list (bools as 0/1, enums as
    /// indices). Used to detect whether a preset has been edited.
    pub fn tone_fields(&self) -> Vec<f32> {
        let b = |v: bool| if v { 1.0 } else { 0.0 };
        vec![
            b(self.gate_enabled),
            self.gate_threshold_db,
            self.wah_mode as u8 as f32,
            self.wah_position,
            self.wah_sensitivity,
            b(self.comp_enabled),
            self.sustain,
            b(self.drive_enabled),
            self.drive,
            self.drive_tone,
            self.drive_level_db,
            b(self.drive_fat),
            self.amp_model as f32,
            self.amp_gain_db,
            self.amp_level_db,
            b(self.cab_enabled),
            self.cab_ir as f32,
            self.cab_low_cut,
            self.cab_high_cut,
            self.eq.bass_db,
            self.eq.mid_db,
            self.eq.mid_freq,
            self.eq.treble_db,
            self.eq.presence_db,
            self.eq.body_db,
            b(self.chorus_enabled),
            self.chorus.rate_hz,
            self.chorus.depth,
            self.chorus.mix,
            b(self.delay_enabled),
            self.delay.time_ms,
            self.delay.feedback,
            self.delay.mix,
            self.delay.tone,
            b(self.delay.ping_pong),
            self.delay.duck,
            self.delay.modulation,
            b(self.reverb_enabled),
            self.reverb.decay_s,
            self.reverb.predelay_ms,
            self.reverb.damping,
            self.reverb.mix,
            b(self.limiter),
        ]
    }
}

/// Values the GUI shows; written once per processed host block.
#[derive(Clone, Copy, Debug, Default)]
pub struct Meters {
    /// Peak of the selected input before trim / guitar offset.
    pub raw_input_peak: f32,
    pub input_peak: f32,
    pub output_peak_l: f32,
    pub output_peak_r: f32,
    pub gate_reduction_db: f32,
    /// Background hiss the gate has measured on the input (RMS dBFS).
    pub noise_floor_db: f32,
    pub comp_reduction_db: f32,
}

/// Soft-knee ceiling: transparent below 0.9, smoothly approaching 1.0.
#[inline]
pub fn safety_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.9 {
        x
    } else {
        x.signum() * (0.9 + 0.1 * ((a - 0.9) / 0.1).tanh())
    }
}

/// Tail-aware bypass for time-based effects: the feed fades, the wet output
/// keeps playing until it has decayed, then processing stops entirely.
struct Trails {
    fade: Fade,
    silent_samples: usize,
    idle_after: usize,
    idle: bool,
}

impl Trails {
    fn new(sr: f32, engaged: bool) -> Self {
        let mut fade = Fade::new(engaged);
        fade.set_time(sr, 10.0);
        Self {
            fade,
            silent_samples: 0,
            idle_after: (sr * 0.5) as usize,
            idle: !engaged,
        }
    }
    fn reset(&mut self, engaged: bool) {
        self.fade.reset(engaged);
        self.silent_samples = 0;
        self.idle = !engaged;
    }
    fn set(&mut self, engaged: bool) {
        self.fade.set_engaged(engaged);
        if engaged {
            self.idle = false;
            self.silent_samples = 0;
        }
    }
    fn report(&mut self, n: usize, wet_peak: f32) -> bool {
        // Returns true when the module just went idle (caller resets it).
        if self.fade.is_off() {
            if wet_peak < 1e-5 {
                self.silent_samples += n;
            } else {
                self.silent_samples = 0;
            }
            if self.silent_samples >= self.idle_after {
                self.idle = true;
                return true;
            }
        }
        false
    }
}

/// Auto input: tells "something is being played on this input" from "this
/// input only carries hiss", per block. The input's own noise floor is
/// learned like the gate's (lowest level heard, rising quickly only while
/// the input sounds like broadband hiss), and the input counts as played
/// while its recent peak level is well above that floor.
#[derive(Clone, Copy)]
struct InputActivity {
    /// Recent level (mean square, peak-held with a 300 ms release).
    level: f32,
    /// Learned hiss level (mean square).
    floor: f32,
}

impl Default for InputActivity {
    fn default() -> Self {
        Self {
            level: 0.0,
            // Starts high and falls to the real hiss at the first quiet block.
            floor: 1e-5,
        }
    }
}

impl InputActivity {
    /// Played this far above its own hiss (20 dB).
    const MARGIN: f32 = 100.0;

    /// Returns (being played, recent level).
    fn update(&mut self, block: &[f32], sr: f32) -> (bool, f32) {
        let n = block.len().max(1);
        let e = block.iter().map(|x| x * x).sum::<f32>() / n as f32;
        let crossings = block.windows(2).filter(|w| (w[0] > 0.0) != (w[1] > 0.0)).count();
        let secs = n as f32 / sr;
        self.level = e.max(self.level * (-secs / 0.3).exp());
        if e < self.floor {
            self.floor = e;
        } else {
            // Hiss crosses zero far more often than any string tone.
            let hiss_like = crossings as f32 / secs > 6_000.0;
            let db_per_s = if hiss_like { 12.0 } else { 0.3 };
            self.floor = (self.floor * 10f32.powf(db_per_s / 10.0 * secs)).min(e);
        }
        self.floor = self.floor.max(1e-14);
        (self.level > self.floor * Self::MARGIN && self.level > 1e-10, self.level)
    }
}

pub struct Engine {
    sr: f32,
    in_gain: Smoothed,
    sel_l: Smoothed,
    sel_r: Smoothed,
    /// Auto input mode has seen a signal since the last reset. Until then
    /// there is nothing to crossfade from, so the first choice applies at once.
    sel_decided: bool,
    /// Auto input mode: is something being played on each input?
    activity: [InputActivity; 2],
    gate: NoiseGate,
    gate_fade: Fade,
    wah: Wah,
    wah_fade: Fade,
    wah_mode_last: WahMode,
    comp: SustainCompressor,
    comp_fade: Fade,
    drive: Overdrive,
    pub amp: AmpStage,
    pub cab: CabStage,
    eq: Eq,
    chorus: Chorus,
    chorus_fade: Fade,
    delay: StereoDelay,
    delay_trails: Trails,
    reverb: Reverb,
    reverb_trails: Trails,
    out_gain: Smoothed,
    mono: Vec<f32>,
    dry: Vec<f32>,
    feed: Vec<f32>,
    feed_l: Vec<f32>,
    feed_r: Vec<f32>,
    pub meters: Meters,
    latency: usize,
}

impl Engine {
    /// Build a complete engine for `sample_rate` (allocates: call off the audio thread).
    /// Amp models and cab IRs are installed separately with `amp.set_slot` / `cab.set_slot`.
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate;
        let amp_latency = amp::stage_latency(sr as f64, MAX_CHUNK);
        let mk = |v: f32, ms: f32| {
            let mut s = Smoothed::new(v);
            s.set_time(sr, ms);
            s
        };
        let fade = |ms: f32| {
            let mut f = Fade::new(false);
            f.set_time(sr, ms);
            f
        };
        Self {
            sr,
            in_gain: mk(1.0, 20.0),
            sel_l: mk(0.5, 30.0),
            sel_r: mk(0.5, 30.0),
            sel_decided: false,
            activity: [InputActivity::default(); 2],
            gate: NoiseGate::new(sr),
            gate_fade: fade(10.0),
            wah: Wah::new(sr),
            wah_fade: fade(15.0),
            wah_mode_last: WahMode::Off,
            comp: SustainCompressor::new(sr),
            comp_fade: fade(15.0),
            drive: Overdrive::new(sr),
            amp: AmpStage::new(sr, MAX_CHUNK, amp_latency),
            cab: CabStage::new(sr, MAX_CHUNK),
            eq: Eq::new(sr),
            chorus: Chorus::new(sr),
            chorus_fade: fade(15.0),
            delay: StereoDelay::new(sr),
            delay_trails: Trails::new(sr, false),
            reverb: Reverb::new(sr),
            reverb_trails: Trails::new(sr, false),
            out_gain: mk(1.0, 20.0),
            mono: vec![0.0; MAX_CHUNK],
            dry: vec![0.0; MAX_CHUNK],
            feed: vec![0.0; MAX_CHUNK],
            feed_l: vec![0.0; MAX_CHUNK],
            feed_r: vec![0.0; MAX_CHUNK],
            meters: Meters::default(),
            latency: oversample::LATENCY + amp_latency,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    /// Total processing latency in samples (constant for a given sample rate).
    pub fn latency(&self) -> usize {
        self.latency
    }

    pub fn amp_latency(&self) -> usize {
        self.amp.latency()
    }

    /// Clear all state and jump every smoother to `p` (no fades on reset).
    pub fn reset(&mut self, p: &ChainParams) {
        self.in_gain
            .reset(db_to_gain(p.input_gain_db));
        let (l, r) = match p.input_mode {
            InputMode::Left => (1.0, 0.0),
            InputMode::Right => (0.0, 1.0),
            InputMode::Auto => (0.5, 0.5),
        };
        self.sel_l.reset(l);
        self.sel_r.reset(r);
        self.sel_decided = false;
        self.activity = [InputActivity::default(); 2];
        self.gate.reset();
        self.gate_fade.reset(p.gate_enabled);
        self.wah.reset(p.wah_position);
        self.wah_fade.reset(p.wah_mode != WahMode::Off);
        self.wah_mode_last = p.wah_mode;
        self.comp.reset(p.sustain);
        self.comp_fade.reset(p.comp_enabled);
        self.drive.reset(
            p.drive_enabled,
            p.drive,
            p.drive_tone,
            p.drive_level_db,
            p.drive_fat,
        );
        self.amp.reset(p.amp_model, p.amp_gain_db, p.amp_level_db);
        self.cab
            .reset(p.cab_enabled, p.cab_ir, p.cab_low_cut, p.cab_high_cut);
        self.eq.reset(&p.eq);
        self.chorus.reset(&p.chorus);
        self.chorus_fade.reset(p.chorus_enabled);
        self.delay.reset(&p.delay);
        self.delay_trails.reset(p.delay_enabled);
        self.reverb.reset(&p.reverb);
        self.reverb_trails.reset(p.reverb_enabled);
        self.out_gain.reset(db_to_gain(p.output_db + MAKEUP_DB));
        self.meters = Meters::default();
    }

    /// Install a freshly built custom amp model (real-time safe hand-over).
    /// Returns a slot that must be dropped off the audio thread.
    pub fn swap_custom_amp(&mut self, slot: Box<NamSlot>) -> Option<Box<NamSlot>> {
        self.amp.swap_custom(slot)
    }

    pub fn swap_custom_cab(&mut self, slot: Box<CabSlot>) -> Option<Box<CabSlot>> {
        self.cab.swap_custom(slot)
    }

    fn select_input(&mut self, left: &[f32], right: Option<&[f32]>, mode: InputMode) {
        let n = left.len();
        let Some(right) = right else {
            for i in 0..n {
                self.mono[i] = sanitize(left[i]);
            }
            return;
        };
        let (tl, tr) = match mode {
            InputMode::Left => (1.0, 0.0),
            InputMode::Right => (0.0, 1.0),
            InputMode::Auto => {
                let el: f32 = left.iter().map(|x| x * x).sum();
                let er: f32 = right.iter().map(|x| x * x).sum();
                let ed: f32 = left.iter().zip(right).map(|(a, b)| (a - b) * (a - b)).sum();
                let sr = self.sr;
                let al = self.activity[0].update(left, sr);
                let ar = self.activity[1].update(right, sr);
                // A single guitar: duplicated on both sides (identical to
                // -60 dB), or on one side with the other digitally silent
                // (60 dB down). Take that channel at once: there is nothing
                // worth crossfading from, and a fade would dip the level (e.g.
                // when a host stops feeding one copy of a duplicated guitar).
                let single = if el > 1e-12 && ed <= el * 1e-6 || er < el * 1e-6 {
                    Some((1.0, 0.0))
                } else if el < er * 1e-6 {
                    Some((0.0, 1.0))
                } else {
                    None
                };
                // Otherwise follow the input(s) something is played on. An
                // input carrying only hiss (an empty instrument input with its
                // gain up) is never mixed in: at amp gain that hiss is loud.
                let chosen = single.or_else(|| match (al.0, ar.0) {
                    (true, false) => Some((1.0, 0.0)),
                    (false, true) => Some((0.0, 1.0)),
                    (true, true) if al.1 > ar.1 * 16.0 => Some((1.0, 0.0)),
                    (true, true) if ar.1 > al.1 * 16.0 => Some((0.0, 1.0)),
                    // Two instruments at similar levels: blend them.
                    (true, true) => Some((0.5, 0.5)),
                    // Nothing played (a pause): stay where we are.
                    (false, false) => None,
                });
                if chosen.is_none() && !self.sel_decided {
                    // Nothing played yet: listen to the quieter input, so an
                    // empty, hissy one doesn't feed the effects before the
                    // first note. Not a decision: the first note still
                    // switches at once.
                    let (l, r) = if self.activity[1].floor < self.activity[0].floor {
                        (0.0, 1.0)
                    } else {
                        (1.0, 0.0)
                    };
                    self.sel_l.reset(l);
                    self.sel_r.reset(r);
                }
                match chosen {
                    Some((l, r)) => {
                        let instant = single.is_some() || !self.sel_decided;
                        self.sel_decided = true;
                        if instant {
                            self.sel_l.reset(l);
                            self.sel_r.reset(r);
                        }
                        (l, r)
                    }
                    None => (self.sel_l.target(), self.sel_r.target()),
                }
            }
        };
        self.sel_l.set_target(tl);
        self.sel_r.set_target(tr);
        for i in 0..n {
            self.mono[i] =
                sanitize(left[i]) * self.sel_l.next() + sanitize(right[i]) * self.sel_r.next();
        }
    }

    /// Process one host block. `right` is `None` for mono-out layouts, in
    /// which case `left` receives the mono sum of the stereo effects.
    pub fn process(&mut self, left: &mut [f32], mut right: Option<&mut [f32]>, p: &ChainParams) {
        let total = left.len();
        let mut meters = Meters::default();
        let mut off = 0;
        while off < total {
            let n = (total - off).min(MAX_CHUNK);
            let l = &mut left[off..off + n];
            match right.as_deref_mut() {
                Some(r) => self.process_chunk(l, Some(&mut r[off..off + n]), p, &mut meters),
                None => self.process_chunk(l, None, p, &mut meters),
            }
            off += n;
        }
        meters.gate_reduction_db = self.gate.reduction_db();
        meters.noise_floor_db = self.gate.noise_floor_db();
        meters.comp_reduction_db = self.comp.gain_reduction_db();
        self.meters = meters;
    }

    fn process_chunk(
        &mut self,
        left: &mut [f32],
        right: Option<&mut [f32]>,
        p: &ChainParams,
        meters: &mut Meters,
    ) {
        let n = left.len();
        self.select_input(left, right.as_deref(), p.input_mode);
        for x in self.mono[..n].iter() {
            meters.raw_input_peak = meters.raw_input_peak.max(x.abs());
        }

        // Input trim.
        self.in_gain.set_target(db_to_gain(p.input_gain_db));
        for x in self.mono[..n].iter_mut() {
            *x *= self.in_gain.next();
            meters.input_peak = meters.input_peak.max(x.abs());
        }

        // Noise gate.
        self.gate_fade.set_engaged(p.gate_enabled);
        if !self.gate_fade.is_off() {
            self.dry[..n].copy_from_slice(&self.mono[..n]);
            self.gate.process(&mut self.mono[..n], p.gate_threshold_db);
            for i in 0..n {
                let a = self.gate_fade.next();
                self.mono[i] = self.dry[i] + (self.mono[i] - self.dry[i]) * a;
            }
        }

        // Wah.
        let wah_on = p.wah_mode != WahMode::Off;
        if wah_on {
            self.wah_mode_last = p.wah_mode;
        }
        self.wah_fade.set_engaged(wah_on);
        if !self.wah_fade.is_off() {
            self.dry[..n].copy_from_slice(&self.mono[..n]);
            self.wah.process(
                &mut self.mono[..n],
                self.wah_mode_last,
                p.wah_position,
                p.wah_sensitivity,
            );
            for i in 0..n {
                let a = self.wah_fade.next();
                self.mono[i] = self.dry[i] + (self.mono[i] - self.dry[i]) * a;
            }
            if self.wah_fade.is_off() {
                self.wah.reset(p.wah_position);
            }
        }

        // Sustain compressor.
        self.comp_fade.set_engaged(p.comp_enabled);
        if !self.comp_fade.is_off() {
            self.dry[..n].copy_from_slice(&self.mono[..n]);
            self.comp.process(&mut self.mono[..n], p.sustain);
            for i in 0..n {
                let a = self.comp_fade.next();
                self.mono[i] = self.dry[i] + (self.mono[i] - self.dry[i]) * a;
            }
        }

        // Overdrive (always present: constant latency).
        self.drive.process(
            &mut self.mono[..n],
            p.drive_enabled,
            p.drive,
            p.drive_tone,
            p.drive_level_db,
            p.drive_fat,
        );

        // Amp, cab, EQ.
        self.amp.process(
            &mut self.mono[..n],
            p.amp_model,
            p.amp_gain_db,
            p.amp_level_db,
        );
        self.cab.process(
            &mut self.mono[..n],
            p.cab_enabled,
            p.cab_ir,
            p.cab_low_cut,
            p.cab_high_cut,
        );
        self.eq.process(&mut self.mono[..n], &p.eq);

        // Stereo effects: start from the dry mono signal in both channels.
        let (out_l, out_r) = (&mut self.feed_l, &mut self.feed_r);
        out_l[..n].copy_from_slice(&self.mono[..n]);
        out_r[..n].copy_from_slice(&self.mono[..n]);

        // Chorus (mono -> stereo). `feed` is free here and holds the fade ramp.
        self.chorus_fade.set_engaged(p.chorus_enabled);
        if !self.chorus_fade.is_off() {
            for i in 0..n {
                self.feed[i] = self.chorus_fade.next();
            }
            self.chorus
                .process(&mut out_l[..n], &mut out_r[..n], &p.chorus, &self.feed[..n]);
            if self.chorus_fade.is_off() {
                self.chorus.reset(&p.chorus);
            }
        }

        self.delay_trails.set(p.delay_enabled);
        if !self.delay_trails.idle {
            for i in 0..n {
                self.feed[i] = self.mono[i] * self.delay_trails.fade.next();
            }
            let peak =
                self.delay
                    .process(&self.feed[..n], &mut out_l[..n], &mut out_r[..n], &p.delay);
            if self.delay_trails.report(n, peak) {
                self.delay.reset(&p.delay);
            }
        }

        self.reverb_trails.set(p.reverb_enabled);
        if !self.reverb_trails.idle {
            // Feed buffers: the current stereo signal (dry + echoes) times the fade.
            // `dry` and `feed` are free here; reuse them as the reverb's feed.
            for i in 0..n {
                let a = self.reverb_trails.fade.next();
                self.dry[i] = out_l[i] * a;
                self.feed[i] = out_r[i] * a;
            }
            let peak = self.reverb.process(
                &self.dry[..n],
                &self.feed[..n],
                &mut out_l[..n],
                &mut out_r[..n],
                &p.reverb,
            );
            if self.reverb_trails.report(n, peak) {
                self.reverb.reset(&p.reverb);
            }
        }

        // Output gain, safety limiter, final NaN guard.
        self.out_gain
            .set_target(db_to_gain(p.output_db + MAKEUP_DB));
        match right {
            Some(right) => {
                for i in 0..n {
                    let g = self.out_gain.next();
                    let (mut a, mut b) = (sanitize(out_l[i] * g), sanitize(out_r[i] * g));
                    if p.limiter {
                        a = safety_limit(a);
                        b = safety_limit(b);
                    }
                    left[i] = a;
                    right[i] = b;
                    meters.output_peak_l = meters.output_peak_l.max(a.abs());
                    meters.output_peak_r = meters.output_peak_r.max(b.abs());
                }
            }
            None => {
                for i in 0..n {
                    let g = self.out_gain.next();
                    let mut a = sanitize(0.5 * (out_l[i] + out_r[i]) * g);
                    if p.limiter {
                        a = safety_limit(a);
                    }
                    left[i] = a;
                    meters.output_peak_l = meters.output_peak_l.max(a.abs());
                    meters.output_peak_r = meters.output_peak_l;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_is_bounded_and_monotonic() {
        let mut last = -2.0f32;
        for i in -2000..=2000 {
            let x = i as f32 / 100.0;
            let y = safety_limit(x);
            assert!(y.abs() <= 1.0);
            assert!(y >= last);
            last = y;
        }
        assert_eq!(safety_limit(0.5), 0.5);
    }
}
