//! Neural amplifier stage: NAM inference via `nam-rs`.
//!
//! A [`NamSlot`] owns one prepared model plus everything needed to run it at
//! the host's rate: an optional sample-rate round trip (NAM captures are only
//! valid at their capture rate) and a padding delay so every slot has the
//! same latency, letting the stage crossfade between models without combing.
//! Slots are built off the audio thread and handed over as `Box`es.

use crate::resample::RoundTrip;
use crate::util::{DcBlocker, Smoothed, db_to_gain};
use nam_rs::{Model, NamModel};

/// NAM's convention: models are normalized so their output loudness for the
/// standardized input is -18 dBFS.
pub const TARGET_LOUDNESS_DB: f32 = -18.0;

/// Model slots. The first four are bundled; the last is user-loaded.
pub const SLOT_CLEAN: usize = 0;
pub const SLOT_CRUNCH: usize = 1;
pub const SLOT_LEAD: usize = 2;
pub const SLOT_METAL: usize = 3;
pub const SLOT_CUSTOM: usize = 4;
pub const NUM_SLOTS: usize = 5;

#[derive(Debug)]
pub struct AmpError(pub String);

impl std::fmt::Display for AmpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AmpError {}

/// Latency (in host samples) that every slot is padded to: the round-trip
/// latency of a 48 kHz model at this host rate. 0 when the host runs at 48 kHz.
pub fn stage_latency(host_rate: f64, max_block: usize) -> usize {
    if (host_rate - 48_000.0).abs() < 0.5 {
        0
    } else {
        RoundTrip::new(host_rate, 48_000.0, max_block).latency()
    }
}

pub struct NamSlot {
    model: Model,
    rt: Option<RoundTrip>,
    pad: Vec<f32>,
    pad_pos: usize,
    norm_gain: f32,
    dc: DcBlocker,
    /// Host samples of silent pre-roll left after a reset (see `reset`).
    warm_left: usize,
    warm_len: usize,
    pub name: String,
    pub model_rate: f64,
    pub latency: usize,
}

impl NamSlot {
    /// Build a slot (allocates; never call on the audio thread).
    pub fn new(
        nam: &NamModel,
        name: &str,
        host_rate: f64,
        max_block: usize,
        target_latency: usize,
    ) -> Result<Self, AmpError> {
        let mut model = Model::from_nam(nam).map_err(|e| AmpError(format!("{name}: {e}")))?;
        model.reset();
        let model_rate = nam.expected_sample_rate();
        let rt = if (host_rate - model_rate).abs() < 0.5 {
            None
        } else {
            Some(RoundTrip::new(host_rate, model_rate, max_block))
        };
        let own = rt.as_ref().map_or(0, RoundTrip::latency);
        let pad_len = target_latency.saturating_sub(own);
        let norm_gain = nam.loudness().map_or(1.0, |l| {
            db_to_gain((TARGET_LOUDNESS_DB - l).clamp(-30.0, 30.0))
        });
        // Pre-roll: the receptive field (in host samples), the converter
        // latency, and enough time for the DC blocker to settle on the
        // model's idle output (~7 time constants of 10 Hz).
        let rf_host = (model.receptive_field() as f64 * host_rate / model_rate).ceil() as usize;
        let warm_len = rf_host + own + pad_len + (0.12 * host_rate) as usize;
        let mut slot = Self {
            model,
            rt,
            pad: vec![0.0; pad_len + 1],
            pad_pos: 0,
            norm_gain,
            dc: DcBlocker::new(host_rate as f32, 10.0),
            warm_left: 0,
            warm_len,
            name: name.to_owned(),
            model_rate,
            latency: own + pad_len,
        };
        slot.reset();
        Ok(slot)
    }

    /// Load and prepare a `.nam` file from disk (off the audio thread).
    pub fn from_file(
        path: &std::path::Path,
        host_rate: f64,
        max_block: usize,
        target_latency: usize,
    ) -> Result<Self, AmpError> {
        let nam =
            NamModel::from_file(path).map_err(|e| AmpError(format!("{}: {e}", path.display())))?;
        let name = nam
            .metadata_typed()
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| {
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        Self::new(&nam, &name, host_rate, max_block, target_latency)
    }

    /// Return to silence. A WaveNet with an all-zero history does not output
    /// silence (its biases produce a transient and a DC offset), so the slot
    /// then pre-rolls silence internally with its output muted until settled.
    pub fn reset(&mut self) {
        self.model.reset();
        if let Some(rt) = &mut self.rt {
            rt.reset();
        }
        self.pad.fill(0.0);
        self.pad_pos = 0;
        self.dc.reset();
        self.warm_left = self.warm_len;
    }

    /// True once the post-reset pre-roll has finished.
    pub fn is_warm(&self) -> bool {
        self.warm_left == 0
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        // The pre-roll ends at an exact sample, independent of block size.
        let muted = self.warm_left.min(buf.len());
        buf[..muted].fill(0.0);
        match &mut self.rt {
            None => self.model.process_buffer(buf),
            Some(rt) => {
                let model = &mut self.model;
                rt.process(buf, |b| model.process_buffer(b));
            }
        }
        let n = self.pad.len();
        if n > 1 {
            for x in buf.iter_mut() {
                self.pad[self.pad_pos] = *x;
                self.pad_pos = (self.pad_pos + 1) % n;
                *x = self.pad[self.pad_pos];
            }
        }
        let g = self.norm_gain;
        for x in buf.iter_mut() {
            // A model should never emit NaN, but a corrupt custom file might.
            let v = if x.is_finite() { *x } else { 0.0 };
            *x = self.dc.process(v) * g;
        }
        buf[..muted].fill(0.0);
        self.warm_left -= muted;
    }
}

/// Which model is producing sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    Slot(usize),
    /// A replaced custom model that keeps playing until its successor is warm.
    Retired,
}

pub struct AmpStage {
    slots: [Option<Box<NamSlot>>; NUM_SLOTS],
    retired: Option<Box<NamSlot>>,
    active: Src,
    /// Slot being warmed up before we crossfade to it.
    pending: Option<usize>,
    fading_from: Option<Src>,
    fade_pos: usize,
    fade_len: usize,
    in_gain: Smoothed,
    out_gain: Smoothed,
    dry: Vec<f32>,
    tmp: Vec<f32>,
    latency: usize,
}

impl AmpStage {
    pub fn new(host_rate: f32, max_block: usize, latency: usize) -> Self {
        let mut in_gain = Smoothed::new(1.0);
        let mut out_gain = Smoothed::new(1.0);
        in_gain.set_time(host_rate, 20.0);
        out_gain.set_time(host_rate, 20.0);
        Self {
            slots: std::array::from_fn(|_| None),
            retired: None,
            active: Src::Slot(SLOT_LEAD),
            pending: None,
            fading_from: None,
            fade_pos: 0,
            fade_len: (host_rate * 0.04) as usize,
            in_gain,
            out_gain,
            dry: vec![0.0; max_block],
            tmp: vec![0.0; max_block],
            latency,
        }
    }

    pub fn latency(&self) -> usize {
        self.latency
    }

    pub fn set_slot(&mut self, idx: usize, slot: Option<Box<NamSlot>>) -> Option<Box<NamSlot>> {
        std::mem::replace(&mut self.slots[idx], slot)
    }

    pub fn has_slot(&self, idx: usize) -> bool {
        self.slots[idx].is_some()
    }

    pub fn slot_name(&self, idx: usize) -> Option<&str> {
        self.slots[idx].as_deref().map(|s| s.name.as_str())
    }

    /// Real-time-safe hand-over of a freshly loaded custom model. Returns a
    /// slot that must be dropped elsewhere, if one can be released right away.
    pub fn swap_custom(&mut self, mut incoming: Box<NamSlot>) -> Option<Box<NamSlot>> {
        incoming.reset();
        if self.active == Src::Slot(SLOT_CUSTOM)
            && self.fading_from.is_none()
            && self.retired.is_none()
        {
            // Keep the old model audible while the new one warms up.
            self.retired = self.slots[SLOT_CUSTOM].replace(incoming);
            self.active = Src::Retired;
            self.pending = Some(SLOT_CUSTOM);
            return None;
        }
        if self.fading_from == Some(Src::Slot(SLOT_CUSTOM)) {
            // Rare: replaced mid-crossfade. Cut the remaining fade-out short.
            self.fading_from = None;
        }
        self.slots[SLOT_CUSTOM].replace(incoming)
    }

    /// Take a retired custom slot once nothing plays it any more.
    pub fn take_retired(&mut self) -> Option<Box<NamSlot>> {
        if self.active != Src::Retired && self.fading_from != Some(Src::Retired) {
            self.retired.take()
        } else {
            None
        }
    }

    fn resolve(&self, wanted: usize) -> usize {
        if self.slots[wanted].is_some() {
            wanted
        } else if self.slots[SLOT_LEAD].is_some() {
            SLOT_LEAD
        } else {
            (0..NUM_SLOTS)
                .find(|&i| self.slots[i].is_some())
                .unwrap_or(wanted)
        }
    }

    pub fn reset(&mut self, wanted: usize, gain_db: f32, level_db: f32) {
        for s in self.slots.iter_mut().flatten() {
            s.reset();
        }
        self.active = Src::Slot(self.resolve(wanted));
        self.pending = None;
        self.fading_from = None;
        self.in_gain.reset(db_to_gain(gain_db));
        self.out_gain.reset(db_to_gain(level_db));
    }

    pub fn active_slot(&self) -> Option<usize> {
        match self.active {
            Src::Slot(i) => Some(i),
            Src::Retired => Some(SLOT_CUSTOM),
        }
    }

    fn get(&mut self, src: Src) -> Option<&mut NamSlot> {
        match src {
            Src::Slot(i) => self.slots[i].as_deref_mut(),
            Src::Retired => self.retired.as_deref_mut(),
        }
    }

    /// `buf.len()` must be <= `max_block`.
    pub fn process(&mut self, buf: &mut [f32], wanted: usize, gain_db: f32, level_db: f32) {
        let n = buf.len();
        let target = self.resolve(wanted);

        // Schedule a switch: warm the target up first, then crossfade.
        if self.fading_from.is_none() {
            let playing_target = match self.active {
                Src::Slot(i) => i == target,
                Src::Retired => false,
            };
            match self.pending {
                None if !playing_target => {
                    if let Some(s) = self.slots[target].as_deref_mut() {
                        s.reset();
                        self.pending = Some(target);
                    }
                }
                Some(p) if p != target => {
                    if playing_target {
                        self.pending = None;
                    } else if let Some(s) = self.slots[target].as_deref_mut() {
                        s.reset();
                        self.pending = Some(target);
                    }
                }
                _ => {}
            }
        }

        self.in_gain.set_target(db_to_gain(gain_db));
        self.out_gain.set_target(db_to_gain(level_db));
        for x in buf.iter_mut() {
            *x *= self.in_gain.next();
        }

        let side = self.pending.is_some() || self.fading_from.is_some();
        if side {
            self.dry[..n].copy_from_slice(buf);
        }
        let active = self.active;
        match self.get(active) {
            Some(s) => s.process(buf),
            None => buf.fill(0.0),
        }

        if let Some(p) = self.pending {
            let mut tmp = std::mem::take(&mut self.tmp);
            tmp[..n].copy_from_slice(&self.dry[..n]);
            let warm = match self.slots[p].as_deref_mut() {
                Some(s) => {
                    s.process(&mut tmp[..n]);
                    s.is_warm()
                }
                None => false,
            };
            self.tmp = tmp;
            if warm {
                self.fading_from = Some(self.active);
                self.active = Src::Slot(p);
                self.pending = None;
                self.fade_pos = 0;
            } else if self.slots[p].is_none() {
                self.pending = None;
            }
        } else if let Some(from) = self.fading_from {
            let mut tmp = std::mem::take(&mut self.tmp);
            tmp[..n].copy_from_slice(&self.dry[..n]);
            match self.get(from) {
                Some(s) => s.process(&mut tmp[..n]),
                None => tmp[..n].fill(0.0),
            }
            for (x, o) in buf.iter_mut().zip(tmp[..n].iter()) {
                let t = (self.fade_pos as f32 / self.fade_len as f32).min(1.0);
                // Equal-power crossfade: the two models' outputs are only loosely correlated.
                let a = (t * std::f32::consts::FRAC_PI_2).sin();
                let b = (t * std::f32::consts::FRAC_PI_2).cos();
                *x = *x * a + *o * b;
                self.fade_pos += 1;
            }
            self.tmp = tmp;
            if self.fade_pos >= self.fade_len {
                self.fading_from = None;
            }
        }

        for x in buf.iter_mut() {
            *x *= self.out_gain.next();
        }
    }
}
