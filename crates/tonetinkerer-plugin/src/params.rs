//! Host-visible parameters. Every control on the GUI is one of these, so all
//! of them are automatable and saved with the project. Parameter smoothing
//! happens inside the DSP engine (per sample), so no nice-plug smoothers.

use nice_plug::prelude::*;
use parking_lot::RwLock;
use std::sync::Arc;
use tonetinkerer_dsp::chain::{ChainParams, GuitarType, InputMode};
use tonetinkerer_dsp::chorus::ChorusSettings;
use tonetinkerer_dsp::delay::DelaySettings;
use tonetinkerer_dsp::eq::EqSettings;
use tonetinkerer_dsp::macros::{self, Macros};
use tonetinkerer_dsp::reverb::ReverbSettings;
use tonetinkerer_dsp::wah::WahMode;
use tonetinkerer_dsp::{amp, cab};

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
pub enum InputSel {
    Auto,
    #[name = "Left (In 1)"]
    Left,
    #[name = "Right (In 2)"]
    Right,
}

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
pub enum GuitarSel {
    #[name = "Single-coil"]
    SingleCoil,
    Humbucker,
    Active,
}

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
pub enum WahSel {
    Off,
    Manual,
    Auto,
}

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
pub enum AmpSel {
    Clean,
    Crunch,
    Lead,
    Metal,
    Custom,
}

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
pub enum CabSel {
    #[name = "4x12 Closed"]
    Closed4x12,
    #[name = "2x12 Open"]
    Open2x12,
    #[name = "1x12 Vintage"]
    Vintage1x12,
    #[name = "4x12 Modern"]
    Modern4x12,
    #[name = "4x12 British"]
    British4x12,
    Custom,
}

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
pub enum DelaySync {
    #[name = "Free (ms)"]
    Free,
    #[name = "1/4"]
    Quarter,
    #[name = "Dotted 1/8"]
    DottedEighth,
    #[name = "1/8"]
    Eighth,
    #[name = "1/4 Triplet"]
    QuarterTriplet,
    #[name = "Dotted 1/4"]
    DottedQuarter,
    #[name = "1/2"]
    Half,
}

impl DelaySync {
    /// Delay length in quarter notes, or None for free time.
    pub fn quarters(self) -> Option<f32> {
        match self {
            DelaySync::Free => None,
            DelaySync::Quarter => Some(1.0),
            DelaySync::DottedEighth => Some(0.75),
            DelaySync::Eighth => Some(0.5),
            DelaySync::QuarterTriplet => Some(2.0 / 3.0),
            DelaySync::DottedQuarter => Some(1.5),
            DelaySync::Half => Some(2.0),
        }
    }
}

fn db(name: &str, default: f32, min: f32, max: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min, max })
        .with_unit(" dB")
        .with_step_size(0.1)
        .with_value_to_string(formatters::v2s_f32_rounded(1))
}

fn unit(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_unit("%")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

/// Macro knob: -1..+1 internally, shown as 0..10 with 5 = "as designed".
fn macro_knob(name: &str) -> FloatParam {
    FloatParam::new(
        name,
        0.0,
        FloatRange::Linear {
            min: -1.0,
            max: 1.0,
        },
    )
    .with_value_to_string(Arc::new(|v| format!("{:.1}", (v + 1.0) * 5.0)))
    .with_string_to_value(Arc::new(|s| {
        s.trim()
            .parse::<f32>()
            .ok()
            .map(|x| (x / 5.0 - 1.0).clamp(-1.0, 1.0))
    }))
}

fn hz(name: &str, default: f32, min: f32, max: f32) -> FloatParam {
    FloatParam::new(
        name,
        default,
        FloatRange::Skewed {
            min,
            max,
            factor: FloatRange::skew_factor(-2.0),
        },
    )
    .with_value_to_string(formatters::v2s_f32_hz_then_khz(0))
    .with_string_to_value(formatters::s2v_f32_hz_then_khz())
}

#[derive(Params)]
pub struct ToneParams {
    // Macros (the Play screen)
    #[id = "m_gain"]
    pub m_gain: FloatParam,
    #[id = "m_body"]
    pub m_body: FloatParam,
    #[id = "m_bright"]
    pub m_bright: FloatParam,
    #[id = "m_mids"]
    pub m_mids: FloatParam,
    #[id = "m_sustain"]
    pub m_sustain: FloatParam,
    #[id = "m_space"]
    pub m_space: FloatParam,
    #[id = "m_volume"]
    pub m_volume: FloatParam,

    // Input
    #[id = "guitar"]
    pub guitar: EnumParam<GuitarSel>,
    #[id = "in_sel"]
    pub input_sel: EnumParam<InputSel>,
    #[id = "in_gain"]
    pub input_gain: FloatParam,

    // Gate
    #[id = "gate_on"]
    pub gate_on: BoolParam,
    #[id = "gate_thr"]
    pub gate_threshold: FloatParam,

    // Wah
    #[id = "wah_mode"]
    pub wah_mode: EnumParam<WahSel>,
    #[id = "wah_pos"]
    pub wah_position: FloatParam,
    #[id = "wah_sens"]
    pub wah_sensitivity: FloatParam,

    // Sustain compressor
    #[id = "comp_on"]
    pub comp_on: BoolParam,
    #[id = "sustain"]
    pub sustain: FloatParam,

    // Overdrive
    #[id = "od_on"]
    pub drive_on: BoolParam,
    #[id = "od_drive"]
    pub drive: FloatParam,
    #[id = "od_tone"]
    pub drive_tone: FloatParam,
    #[id = "od_level"]
    pub drive_level: FloatParam,
    #[id = "od_fat"]
    pub drive_fat: BoolParam,

    // Amp
    #[id = "amp_model"]
    pub amp_model: EnumParam<AmpSel>,
    #[id = "amp_gain"]
    pub amp_gain: FloatParam,
    #[id = "amp_level"]
    pub amp_level: FloatParam,

    // Cab
    #[id = "cab_on"]
    pub cab_on: BoolParam,
    #[id = "cab_ir"]
    pub cab_ir: EnumParam<CabSel>,
    #[id = "cab_lc"]
    pub cab_low_cut: FloatParam,
    #[id = "cab_hc"]
    pub cab_high_cut: FloatParam,

    // EQ
    #[id = "eq_bass"]
    pub bass: FloatParam,
    #[id = "eq_mid"]
    pub mid: FloatParam,
    #[id = "eq_midf"]
    pub mid_freq: FloatParam,
    #[id = "eq_treb"]
    pub treble: FloatParam,
    #[id = "eq_pres"]
    pub presence: FloatParam,
    #[id = "eq_body"]
    pub body: FloatParam,

    // Chorus
    #[id = "cho_on"]
    pub chorus_on: BoolParam,
    #[id = "cho_rate"]
    pub chorus_rate: FloatParam,
    #[id = "cho_depth"]
    pub chorus_depth: FloatParam,
    #[id = "cho_mix"]
    pub chorus_mix: FloatParam,

    // Delay
    #[id = "dly_on"]
    pub delay_on: BoolParam,
    #[id = "dly_sync"]
    pub delay_sync: EnumParam<DelaySync>,
    #[id = "dly_time"]
    pub delay_time: FloatParam,
    #[id = "dly_fb"]
    pub delay_feedback: FloatParam,
    #[id = "dly_mix"]
    pub delay_mix: FloatParam,
    #[id = "dly_tone"]
    pub delay_tone: FloatParam,
    #[id = "dly_pp"]
    pub delay_ping_pong: BoolParam,
    #[id = "dly_duck"]
    pub delay_duck: FloatParam,
    #[id = "dly_mod"]
    pub delay_mod: FloatParam,

    // Reverb
    #[id = "rev_on"]
    pub reverb_on: BoolParam,
    #[id = "rev_decay"]
    pub reverb_decay: FloatParam,
    #[id = "rev_pre"]
    pub reverb_predelay: FloatParam,
    #[id = "rev_damp"]
    pub reverb_damping: FloatParam,
    #[id = "rev_mix"]
    pub reverb_mix: FloatParam,

    // Output
    #[id = "out_gain"]
    pub output: FloatParam,
    #[id = "limiter"]
    pub limiter: BoolParam,

    /// Path of the user's `.nam` model, restored with the project.
    #[persist = "custom_amp_path"]
    pub custom_amp_path: RwLock<Option<String>>,
    /// Path of the user's cabinet IR `.wav`, restored with the project.
    #[persist = "custom_ir_path"]
    pub custom_ir_path: RwLock<Option<String>>,
    /// Name of the last applied factory preset (display only).
    #[persist = "preset_name"]
    pub preset_name: RwLock<String>,
}

impl Default for ToneParams {
    fn default() -> Self {
        let d = (tonetinkerer_dsp::presets::PRESETS[0].params)();
        Self {
            m_gain: macro_knob("Gain"),
            m_body: macro_knob("Body"),
            m_bright: macro_knob("Bright"),
            m_mids: macro_knob("Mids"),
            m_sustain: macro_knob("Sustain"),
            m_space: macro_knob("Space"),
            m_volume: db("Volume", 0.0, -12.0, 12.0),
            guitar: EnumParam::new("Guitar", GuitarSel::Humbucker),
            input_sel: EnumParam::new("Input", InputSel::Auto),
            input_gain: db("Input Trim", d.input_gain_db, -24.0, 24.0),
            gate_on: BoolParam::new("Gate", d.gate_enabled),
            gate_threshold: db("Gate Threshold", d.gate_threshold_db, -96.0, -20.0),
            wah_mode: EnumParam::new("Wah", wah_sel(d.wah_mode)),
            wah_position: unit("Wah Position", d.wah_position),
            wah_sensitivity: unit("Wah Sensitivity", d.wah_sensitivity),
            comp_on: BoolParam::new("Sustain On", d.comp_enabled),
            sustain: unit("Compressor", d.sustain),
            drive_on: BoolParam::new("Drive On", d.drive_enabled),
            drive: unit("Drive", d.drive),
            drive_tone: unit("Drive Tone", d.drive_tone),
            drive_level: db("Drive Level", d.drive_level_db, -12.0, 12.0),
            drive_fat: BoolParam::new("Drive Fat", d.drive_fat),
            amp_model: EnumParam::new("Amp Model", amp_sel(d.amp_model)),
            amp_gain: db("Amp Gain", d.amp_gain_db, -18.0, 18.0),
            amp_level: db("Amp Level", d.amp_level_db, -24.0, 12.0),
            cab_on: BoolParam::new("Cab On", d.cab_enabled),
            cab_ir: EnumParam::new("Cabinet", cab_sel(d.cab_ir)),
            cab_low_cut: hz("Low Cut", d.cab_low_cut, 20.0, 400.0),
            cab_high_cut: hz("High Cut", d.cab_high_cut, 2_000.0, 20_000.0),
            bass: db("Bass", d.eq.bass_db, -12.0, 12.0),
            mid: db("Mid", d.eq.mid_db, -12.0, 12.0),
            mid_freq: hz("Mid Freq", d.eq.mid_freq, 200.0, 4_000.0),
            treble: db("Treble", d.eq.treble_db, -12.0, 12.0),
            presence: db("Presence", d.eq.presence_db, -12.0, 12.0),
            body: db("Body EQ", d.eq.body_db, -12.0, 12.0),
            chorus_on: BoolParam::new("Chorus On", d.chorus_enabled),
            chorus_rate: FloatParam::new(
                "Chorus Rate",
                d.chorus.rate_hz,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 5.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            chorus_depth: unit("Chorus Depth", d.chorus.depth),
            chorus_mix: unit("Chorus Mix", d.chorus.mix),
            delay_on: BoolParam::new("Delay On", d.delay_enabled),
            delay_sync: EnumParam::new("Delay Sync", DelaySync::Free),
            delay_time: FloatParam::new(
                "Delay Time",
                d.delay.time_ms,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 2_000.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms")
            .with_step_size(1.0),
            delay_feedback: unit("Delay Feedback", d.delay.feedback),
            delay_mix: unit("Delay Mix", d.delay.mix),
            delay_tone: unit("Delay Tone", d.delay.tone),
            delay_ping_pong: BoolParam::new("Ping-Pong", d.delay.ping_pong),
            delay_duck: unit("Delay Duck", d.delay.duck),
            delay_mod: unit("Delay Mod", d.delay.modulation),
            reverb_on: BoolParam::new("Reverb On", d.reverb_enabled),
            reverb_decay: FloatParam::new(
                "Reverb Decay",
                d.reverb.decay_s,
                FloatRange::Skewed {
                    min: 0.3,
                    max: 12.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            reverb_predelay: FloatParam::new(
                "Pre-Delay",
                d.reverb.predelay_ms,
                FloatRange::Linear {
                    min: 0.0,
                    max: 200.0,
                },
            )
            .with_unit(" ms")
            .with_step_size(1.0),
            reverb_damping: unit("Reverb Damping", d.reverb.damping),
            reverb_mix: unit("Reverb Mix", d.reverb.mix),
            output: db("Output", d.output_db, -24.0, 12.0),
            limiter: BoolParam::new("Limiter", d.limiter),
            custom_amp_path: RwLock::new(None),
            custom_ir_path: RwLock::new(None),
            preset_name: RwLock::new(tonetinkerer_dsp::presets::PRESETS[0].name.to_owned()),
        }
    }
}

pub fn amp_index(a: AmpSel) -> usize {
    match a {
        AmpSel::Clean => amp::SLOT_CLEAN,
        AmpSel::Crunch => amp::SLOT_CRUNCH,
        AmpSel::Lead => amp::SLOT_LEAD,
        AmpSel::Metal => amp::SLOT_METAL,
        AmpSel::Custom => amp::SLOT_CUSTOM,
    }
}

pub fn amp_sel(i: usize) -> AmpSel {
    match i {
        amp::SLOT_CLEAN => AmpSel::Clean,
        amp::SLOT_CRUNCH => AmpSel::Crunch,
        amp::SLOT_METAL => AmpSel::Metal,
        amp::SLOT_CUSTOM => AmpSel::Custom,
        _ => AmpSel::Lead,
    }
}

pub fn cab_index(c: CabSel) -> usize {
    match c {
        CabSel::Closed4x12 => cab::SLOT_4X12,
        CabSel::Open2x12 => cab::SLOT_2X12,
        CabSel::Vintage1x12 => cab::SLOT_1X12,
        CabSel::Modern4x12 => cab::SLOT_4X12_MODERN,
        CabSel::British4x12 => cab::SLOT_4X12_BRITISH,
        CabSel::Custom => cab::SLOT_CUSTOM,
    }
}

pub fn cab_sel(i: usize) -> CabSel {
    match i {
        cab::SLOT_2X12 => CabSel::Open2x12,
        cab::SLOT_1X12 => CabSel::Vintage1x12,
        cab::SLOT_4X12_MODERN => CabSel::Modern4x12,
        cab::SLOT_4X12_BRITISH => CabSel::British4x12,
        cab::SLOT_CUSTOM => CabSel::Custom,
        _ => CabSel::Closed4x12,
    }
}

fn wah(w: WahSel) -> WahMode {
    match w {
        WahSel::Off => WahMode::Off,
        WahSel::Manual => WahMode::Manual,
        WahSel::Auto => WahMode::Auto,
    }
}

fn wah_sel(w: WahMode) -> WahSel {
    match w {
        WahMode::Off => WahSel::Off,
        WahMode::Manual => WahSel::Manual,
        WahMode::Auto => WahSel::Auto,
    }
}

impl ToneParams {
    pub fn macros(&self) -> Macros {
        Macros {
            gain: self.m_gain.value(),
            body: self.m_body.value(),
            bright: self.m_bright.value(),
            mids: self.m_mids.value(),
            sustain: self.m_sustain.value(),
            space: self.m_space.value(),
            volume_db: self.m_volume.value(),
        }
    }

    /// Effective DSP parameters: the detailed settings with the macros
    /// applied (real-time safe, no allocation). `tempo` is the host tempo,
    /// used when the delay is tempo-synced.
    pub fn chain_params(&self, tempo: Option<f64>, out: &mut ChainParams) {
        self.base_params(tempo, out);
        let m = self.macros();
        if !m.is_neutral() {
            *out = macros::apply(out, &m);
        }
    }

    /// The detailed settings only (what a sound stores), without macros.
    pub fn base_params(&self, tempo: Option<f64>, out: &mut ChainParams) {
        out.guitar = match self.guitar.value() {
            GuitarSel::SingleCoil => GuitarType::SingleCoil,
            GuitarSel::Humbucker => GuitarType::Humbucker,
            GuitarSel::Active => GuitarType::Active,
        };
        out.input_mode = match self.input_sel.value() {
            InputSel::Auto => InputMode::Auto,
            InputSel::Left => InputMode::Left,
            InputSel::Right => InputMode::Right,
        };
        out.input_gain_db = self.input_gain.value();
        out.gate_enabled = self.gate_on.value();
        out.gate_threshold_db = self.gate_threshold.value();
        out.wah_mode = wah(self.wah_mode.value());
        out.wah_position = self.wah_position.value();
        out.wah_sensitivity = self.wah_sensitivity.value();
        out.comp_enabled = self.comp_on.value();
        out.sustain = self.sustain.value();
        out.drive_enabled = self.drive_on.value();
        out.drive = self.drive.value();
        out.drive_tone = self.drive_tone.value();
        out.drive_level_db = self.drive_level.value();
        out.drive_fat = self.drive_fat.value();
        out.amp_model = amp_index(self.amp_model.value());
        out.amp_gain_db = self.amp_gain.value();
        out.amp_level_db = self.amp_level.value();
        out.cab_enabled = self.cab_on.value();
        out.cab_ir = cab_index(self.cab_ir.value());
        out.cab_low_cut = self.cab_low_cut.value();
        out.cab_high_cut = self.cab_high_cut.value();
        out.eq = EqSettings {
            bass_db: self.bass.value(),
            mid_db: self.mid.value(),
            mid_freq: self.mid_freq.value(),
            treble_db: self.treble.value(),
            presence_db: self.presence.value(),
            body_db: self.body.value(),
        };
        out.chorus_enabled = self.chorus_on.value();
        out.chorus = ChorusSettings {
            rate_hz: self.chorus_rate.value(),
            depth: self.chorus_depth.value(),
            mix: self.chorus_mix.value(),
        };
        out.delay_enabled = self.delay_on.value();
        let time_ms = match (self.delay_sync.value().quarters(), tempo) {
            (Some(q), Some(bpm)) if bpm > 1.0 => (q * 60_000.0 / bpm as f32).min(2_400.0),
            _ => self.delay_time.value(),
        };
        out.delay = DelaySettings {
            time_ms,
            feedback: self.delay_feedback.value(),
            mix: self.delay_mix.value(),
            tone: self.delay_tone.value(),
            ping_pong: self.delay_ping_pong.value(),
            duck: self.delay_duck.value(),
            modulation: self.delay_mod.value(),
        };
        out.reverb_enabled = self.reverb_on.value();
        out.reverb = ReverbSettings {
            decay_s: self.reverb_decay.value(),
            predelay_ms: self.reverb_predelay.value(),
            damping: self.reverb_damping.value(),
            mix: self.reverb_mix.value(),
        };
        out.output_db = self.output.value();
        out.limiter = self.limiter.value();
    }
}

/// Apply a full set of DSP parameters through the host (so preset changes
/// are undoable / recorded as automation like any knob move).
///
/// Input selection, input trim and output level are deliberately left alone:
/// they calibrate the user's interface and mix, not the tone.
pub fn apply_chain_params(setter: &ParamSetter, p: &ToneParams, c: &ChainParams) {
    fn set<P: Param>(s: &ParamSetter, param: &P, v: P::Plain) {
        s.begin_set_parameter(param);
        s.set_parameter(param, v);
        s.end_set_parameter(param);
    }
    set(setter, &p.gate_on, c.gate_enabled);
    set(setter, &p.gate_threshold, c.gate_threshold_db);
    set(setter, &p.wah_mode, wah_sel(c.wah_mode));
    set(setter, &p.wah_position, c.wah_position);
    set(setter, &p.wah_sensitivity, c.wah_sensitivity);
    set(setter, &p.comp_on, c.comp_enabled);
    set(setter, &p.sustain, c.sustain);
    set(setter, &p.drive_on, c.drive_enabled);
    set(setter, &p.drive, c.drive);
    set(setter, &p.drive_tone, c.drive_tone);
    set(setter, &p.drive_level, c.drive_level_db);
    set(setter, &p.drive_fat, c.drive_fat);
    set(setter, &p.amp_model, amp_sel(c.amp_model));
    set(setter, &p.amp_gain, c.amp_gain_db);
    set(setter, &p.amp_level, c.amp_level_db);
    set(setter, &p.cab_on, c.cab_enabled);
    set(setter, &p.cab_ir, cab_sel(c.cab_ir));
    set(setter, &p.cab_low_cut, c.cab_low_cut);
    set(setter, &p.cab_high_cut, c.cab_high_cut);
    set(setter, &p.bass, c.eq.bass_db);
    set(setter, &p.mid, c.eq.mid_db);
    set(setter, &p.mid_freq, c.eq.mid_freq);
    set(setter, &p.treble, c.eq.treble_db);
    set(setter, &p.presence, c.eq.presence_db);
    set(setter, &p.body, c.eq.body_db);
    set(setter, &p.chorus_on, c.chorus_enabled);
    set(setter, &p.chorus_rate, c.chorus.rate_hz);
    set(setter, &p.chorus_depth, c.chorus.depth);
    set(setter, &p.chorus_mix, c.chorus.mix);
    // A new sound starts exactly as designed.
    for m in [
        &p.m_gain,
        &p.m_body,
        &p.m_bright,
        &p.m_mids,
        &p.m_sustain,
        &p.m_space,
    ] {
        set(setter, m, 0.0);
    }
    set(setter, &p.delay_on, c.delay_enabled);
    set(setter, &p.delay_sync, DelaySync::Free);
    set(setter, &p.delay_time, c.delay.time_ms);
    set(setter, &p.delay_feedback, c.delay.feedback);
    set(setter, &p.delay_mix, c.delay.mix);
    set(setter, &p.delay_tone, c.delay.tone);
    set(setter, &p.delay_ping_pong, c.delay.ping_pong);
    set(setter, &p.delay_duck, c.delay.duck);
    set(setter, &p.delay_mod, c.delay.modulation);
    set(setter, &p.reverb_on, c.reverb_enabled);
    set(setter, &p.reverb_decay, c.reverb.decay_s);
    set(setter, &p.reverb_predelay, c.reverb.predelay_ms);
    set(setter, &p.reverb_damping, c.reverb.damping);
    set(setter, &p.reverb_mix, c.reverb.mix);
    set(setter, &p.limiter, c.limiter);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonetinkerer_dsp::presets::PRESETS;

    #[test]
    fn defaults_are_the_first_preset() {
        let p = ToneParams::default();
        let mut c = ChainParams::default();
        p.chain_params(None, &mut c);
        let want = (PRESETS[0].params)();
        assert!(p.macros().is_neutral());
        assert_eq!(c.tone_fields(), want.tone_fields());
    }

    /// Every preset value must survive the host parameter's range and
    /// stepping unchanged, otherwise applying a preset silently alters it.
    #[test]
    fn preset_values_are_representable() {
        let p = ToneParams::default();
        fn check(param: &FloatParam, v: f32, preset: &str) {
            let norm = param.preview_normalized(v);
            assert!(
                (0.0..=1.0).contains(&norm),
                "{preset}: {} = {v} out of range",
                param.name()
            );
            let back = param.preview_plain(norm);
            assert!(
                (back - v).abs() <= 1e-3 * v.abs().max(1.0),
                "{preset}: {} = {v} becomes {back}",
                param.name()
            );
        }
        for preset in PRESETS {
            let c = (preset.params)();
            let n = preset.name;
            check(&p.gate_threshold, c.gate_threshold_db, n);
            check(&p.wah_position, c.wah_position, n);
            check(&p.wah_sensitivity, c.wah_sensitivity, n);
            check(&p.sustain, c.sustain, n);
            check(&p.drive, c.drive, n);
            check(&p.drive_tone, c.drive_tone, n);
            check(&p.drive_level, c.drive_level_db, n);
            check(&p.amp_gain, c.amp_gain_db, n);
            check(&p.amp_level, c.amp_level_db, n);
            check(&p.cab_low_cut, c.cab_low_cut, n);
            check(&p.cab_high_cut, c.cab_high_cut, n);
            check(&p.bass, c.eq.bass_db, n);
            check(&p.mid, c.eq.mid_db, n);
            check(&p.mid_freq, c.eq.mid_freq, n);
            check(&p.treble, c.eq.treble_db, n);
            check(&p.presence, c.eq.presence_db, n);
            check(&p.body, c.eq.body_db, n);
            check(&p.chorus_rate, c.chorus.rate_hz, n);
            check(&p.chorus_depth, c.chorus.depth, n);
            check(&p.chorus_mix, c.chorus.mix, n);
            check(&p.delay_time, c.delay.time_ms, n);
            check(&p.delay_feedback, c.delay.feedback, n);
            check(&p.delay_mix, c.delay.mix, n);
            check(&p.delay_tone, c.delay.tone, n);
            check(&p.delay_duck, c.delay.duck, n);
            check(&p.delay_mod, c.delay.modulation, n);
            check(&p.reverb_decay, c.reverb.decay_s, n);
            check(&p.reverb_predelay, c.reverb.predelay_ms, n);
            check(&p.reverb_damping, c.reverb.damping, n);
            check(&p.reverb_mix, c.reverb.mix, n);
        }
    }
}
