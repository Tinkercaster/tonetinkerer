//! ToneTinkerer: a neural guitar amplifier simulator plugin (VST3 + CLAP).
//!
//! Threading model
//! - Audio thread: `process()` only reads parameters, pops prepared model /
//!   IR slots from a lock-free ring buffer and pushes replaced ones into a
//!   second ring buffer ("trash") so they are freed elsewhere. No allocation,
//!   locking, file I/O or logging happens there.
//! - Background thread: loads and prepares user `.nam` models and IR files.
//! - Main thread: `activate()` (re)builds the whole engine for the host's
//!   sample rate, including any persisted custom model / IR.

use atomic_float::AtomicF32;
use nice_plug::prelude::*;
use nice_plug_egui::{EguiEditorState, RepaintNotifier};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tonetinkerer_dsp::amp::NamSlot;
use tonetinkerer_dsp::assets;
use tonetinkerer_dsp::cab::CabSlot;
use tonetinkerer_dsp::chain::{ChainParams, Engine};

mod editor;
pub mod params;
mod play;
mod skin;
#[cfg(test)]
mod snapshot;
mod widgets;

pub use params::ToneParams;

/// Messages from the loader to the audio thread. Tagged with the engine
/// generation they were prepared for; stale ones are discarded.
pub enum ToAudio {
    Amp(u64, Box<NamSlot>),
    Cab(u64, Box<CabSlot>),
}

/// Objects the audio thread hands back for deallocation.
pub enum Trash {
    Amp(Box<NamSlot>),
    Cab(Box<CabSlot>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    LoadAmp,
    LoadIr,
}

/// Status of a user-loaded file, shown in the GUI.
#[derive(Debug, Clone, Default)]
pub struct LoadStatus {
    pub name: Option<String>,
    pub error: Option<String>,
}

/// State shared between the audio thread, the loader and the GUI.
pub struct Shared {
    pub input_peak: AtomicF32,
    /// Peak of the unprocessed input (for 'Set up my guitar').
    pub raw_input_peak: AtomicF32,
    pub output_peak_l: AtomicF32,
    pub output_peak_r: AtomicF32,
    pub gate_reduction_db: AtomicF32,
    pub noise_floor_db: AtomicF32,
    pub comp_reduction_db: AtomicF32,
    pub sample_rate: AtomicF32,
    pub amp_latency: AtomicUsize,
    pub latency: AtomicUsize,
    pub generation: AtomicU64,
    pub amp_status: Mutex<LoadStatus>,
    pub ir_status: Mutex<LoadStatus>,
    to_audio: Mutex<rtrb::Producer<ToAudio>>,
    trash: Mutex<rtrb::Consumer<Trash>>,
}

impl Shared {
    /// Free anything the audio thread handed back. Never call on the audio thread.
    pub fn empty_trash(&self) {
        if let Some(mut rx) = self.trash.try_lock() {
            while rx.pop().is_ok() {}
        }
    }
}

pub struct ToneTinkerer {
    params: Arc<ToneParams>,
    shared: Arc<Shared>,
    engine: Option<Box<Engine>>,
    /// Engine kept through `deactivate`, with its sample rate and when it was
    /// parked. Hosts restart a plugin for bookkeeping (CLAP: switching to
    /// offline rendering) sometimes mid-render; resuming the same engine
    /// keeps that restart inaudible instead of cutting every tail and note.
    parked: Option<(Box<Engine>, f32, std::time::Instant)>,
    /// Skip the reset the framework issues when processing restarts after
    /// such a seamless re-activation.
    resumed: bool,
    /// The host transport was playing in the last processed block. Only a
    /// restart during playback must be inaudible; one while stopped (e.g.
    /// just before a render) resets as usual so the render starts clean.
    was_playing: bool,
    chain: ChainParams,
    from_loader: rtrb::Consumer<ToAudio>,
    trash: rtrb::Producer<Trash>,
    editor_state: Arc<EguiEditorState>,
    repaint: RepaintNotifier,
    generation: u64,
}

impl Default for ToneTinkerer {
    fn default() -> Self {
        let (to_audio, from_loader) = rtrb::RingBuffer::new(16);
        let (trash_tx, trash_rx) = rtrb::RingBuffer::new(64);
        let shared = Arc::new(Shared {
            input_peak: AtomicF32::new(0.0),
            raw_input_peak: AtomicF32::new(0.0),
            output_peak_l: AtomicF32::new(0.0),
            output_peak_r: AtomicF32::new(0.0),
            gate_reduction_db: AtomicF32::new(0.0),
            noise_floor_db: AtomicF32::new(-120.0),
            comp_reduction_db: AtomicF32::new(0.0),
            sample_rate: AtomicF32::new(48_000.0),
            amp_latency: AtomicUsize::new(0),
            latency: AtomicUsize::new(0),
            generation: AtomicU64::new(0),
            amp_status: Mutex::new(LoadStatus::default()),
            ir_status: Mutex::new(LoadStatus::default()),
            to_audio: Mutex::new(to_audio),
            trash: Mutex::new(trash_rx),
        });
        Self {
            params: Arc::new(ToneParams::default()),
            shared,
            engine: None,
            parked: None,
            resumed: false,
            was_playing: false,
            chain: ChainParams::default(),
            from_loader,
            trash: trash_tx,
            editor_state: EguiEditorState::from_size(editor::WINDOW_SIZE, 1.0),
            repaint: RepaintNotifier::new(),
            generation: 0,
        }
    }
}

/// Load the persisted custom model (if any) for the given rate. Off the audio thread.
fn load_custom_amp(
    params: &ToneParams,
    shared: &Shared,
    sr: f32,
    amp_latency: usize,
) -> Option<Box<NamSlot>> {
    let path = params.custom_amp_path.read().clone()?;
    match assets::custom_amp_slot(std::path::Path::new(&path), sr, amp_latency) {
        Ok(slot) => {
            *shared.amp_status.lock() = LoadStatus {
                name: Some(slot.name.clone()),
                error: None,
            };
            Some(Box::new(slot))
        }
        Err(e) => {
            *shared.amp_status.lock() = LoadStatus {
                name: None,
                error: Some(e.to_string()),
            };
            None
        }
    }
}

fn load_custom_ir(params: &ToneParams, shared: &Shared, sr: f32) -> Option<Box<CabSlot>> {
    let path = params.custom_ir_path.read().clone()?;
    match CabSlot::from_file(std::path::Path::new(&path), sr) {
        Ok(slot) => {
            *shared.ir_status.lock() = LoadStatus {
                name: Some(slot.name.clone()),
                error: None,
            };
            Some(Box::new(slot))
        }
        Err(e) => {
            *shared.ir_status.lock() = LoadStatus {
                name: None,
                error: Some(e.to_string()),
            };
            None
        }
    }
}

impl Plugin for ToneTinkerer {
    const NAME: &'static str = "ToneTinkerer";
    const VENDOR: &'static str = "GuitarWiring";
    const URL: &'static str = "https://guitarwiring.app/";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type Editor = nice_plug_egui::EguiEditor<editor::ToneEditor>;
    type SysExMessage = ();
    type BackgroundTask = Task;

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn task_executor(&mut self) -> TaskExecutor<Self> {
        let params = self.params.clone();
        let shared = self.shared.clone();
        Box::new(move |task| {
            shared.empty_trash();
            let sr = shared.sample_rate.load(Ordering::Acquire);
            let generation = shared.generation.load(Ordering::Acquire);
            let msg = match task {
                Task::LoadAmp => load_custom_amp(
                    &params,
                    &shared,
                    sr,
                    shared.amp_latency.load(Ordering::Acquire),
                )
                .map(|s| ToAudio::Amp(generation, s)),
                Task::LoadIr => {
                    load_custom_ir(&params, &shared, sr).map(|s| ToAudio::Cab(generation, s))
                }
            };
            if let Some(msg) = msg {
                // If the ring is somehow full the slot is simply dropped here, off the audio thread.
                let _ = shared.to_audio.lock().push(msg);
            }
        })
    }

    fn editor(&mut self, async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        editor::create(
            self.params.clone(),
            self.shared.clone(),
            self.editor_state.clone(),
            self.repaint.clone(),
            async_executor,
        )
    }

    fn activate(
        &mut self,
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl ActivateContext<Self>,
    ) -> bool {
        let sr = buffer_config.sample_rate;
        // A quick restart at the same sample rate resumes the running engine
        // (the engine works in fixed internal chunks, so the host's block size
        // doesn't matter). Anything else builds from scratch.
        if let Some((engine, parked_sr, at)) = self.parked.take() {
            if parked_sr == sr && at.elapsed() < std::time::Duration::from_secs(1) {
                context.set_latency_samples(engine.latency() as u32);
                self.engine = Some(engine);
                self.resumed = self.was_playing;
                return true;
            }
        }
        self.resumed = false;
        // Discard anything prepared for a previous configuration.
        while self.from_loader.pop().is_ok() {}
        self.shared.empty_trash();

        let mut engine = Box::new(assets::build_engine(sr));
        let amp_latency = engine.amp_latency();
        if let Some(slot) = load_custom_amp(&self.params, &self.shared, sr, amp_latency) {
            let _ = engine.swap_custom_amp(slot);
        }
        if let Some(slot) = load_custom_ir(&self.params, &self.shared, sr) {
            let _ = engine.swap_custom_cab(slot);
        }
        self.params.chain_params(None, &mut self.chain);
        engine.reset(&self.chain);

        self.generation += 1;
        self.shared.sample_rate.store(sr, Ordering::Release);
        self.shared
            .amp_latency
            .store(amp_latency, Ordering::Release);
        self.shared
            .latency
            .store(engine.latency(), Ordering::Release);
        self.shared
            .generation
            .store(self.generation, Ordering::Release);
        context.set_latency_samples(engine.latency() as u32);
        self.engine = Some(engine);
        true
    }

    fn reset(&mut self) {
        // The framework resets when processing restarts; after a seamless
        // re-activation that would undo the point of resuming.
        if self.resumed {
            return;
        }
        if let Some(engine) = self.engine.as_mut() {
            self.params.chain_params(None, &mut self.chain);
            engine.reset(&self.chain);
        }
    }

    fn deactivate(&mut self) {
        if let Some(engine) = self.engine.take() {
            let sr = self.shared.sample_rate.load(Ordering::Acquire);
            self.parked = Some((engine, sr, std::time::Instant::now()));
        }
        self.shared.empty_trash();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Only the reset right after a resume is skipped; later ones are real.
        self.resumed = false;
        self.was_playing = context.transport().playing;
        let Some(engine) = self.engine.as_mut() else {
            return ProcessStatus::Normal;
        };

        // Real-time-safe hand-over of freshly loaded models / IRs.
        while let Ok(msg) = self.from_loader.pop() {
            let old = match msg {
                ToAudio::Amp(g, slot) if g == self.generation => {
                    engine.swap_custom_amp(slot).map(Trash::Amp)
                }
                ToAudio::Amp(_, slot) => Some(Trash::Amp(slot)),
                ToAudio::Cab(g, slot) if g == self.generation => {
                    engine.swap_custom_cab(slot).map(Trash::Cab)
                }
                ToAudio::Cab(_, slot) => Some(Trash::Cab(slot)),
            };
            if let Some(t) = old {
                // A full trash ring (practically impossible) means we free here.
                let _ = self.trash.push(t);
            }
        }
        if let Some(r) = engine.amp.take_retired() {
            let _ = self.trash.push(Trash::Amp(r));
        }

        self.params
            .chain_params(context.transport().tempo, &mut self.chain);

        let channels = buffer.as_slice();
        match channels {
            [l] => engine.process(l, None, &self.chain),
            [l, r, ..] => engine.process(l, Some(r), &self.chain),
            [] => {}
        }

        let m = engine.meters;
        let s = &self.shared;
        // Peak-hold with decay is done in the GUI; here we only publish maxima.
        s.input_peak.fetch_max(m.input_peak, Ordering::Relaxed);
        s.raw_input_peak
            .fetch_max(m.raw_input_peak, Ordering::Relaxed);
        s.output_peak_l
            .fetch_max(m.output_peak_l, Ordering::Relaxed);
        s.output_peak_r
            .fetch_max(m.output_peak_r, Ordering::Relaxed);
        s.gate_reduction_db
            .store(m.gate_reduction_db, Ordering::Relaxed);
        s.noise_floor_db
            .store(m.noise_floor_db, Ordering::Relaxed);
        s.comp_reduction_db
            .store(m.comp_reduction_db, Ordering::Relaxed);
        if self.editor_state.is_open() {
            self.repaint.request_repaint();
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for ToneTinkerer {
    const CLAP_ID: &'static str = "app.guitarwiring.tonetinkerer";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Neural guitar amplifier simulator for singing lead tones");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Distortion,
    ];
}

impl Vst3Plugin for ToneTinkerer {
    const VST3_CLASS_ID: [u8; 16] = *b"ToneTinkererAmp1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Distortion,
        Vst3SubCategory::Stereo,
    ];
}

nice_export_clap!(ToneTinkerer);
nice_export_vst3!(ToneTinkerer);
