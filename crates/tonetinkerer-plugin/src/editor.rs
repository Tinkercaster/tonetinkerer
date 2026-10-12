//! The ToneTinkerer editor. Two views share one window:
//! - Play (default, `play.rs`): pick a sound, adjust plain-language macros.
//! - Advanced: the full signal chain (dynamics, wah, drive, amp, cabinet,
//!   chorus, delay, reverb) with every detailed control.

use crate::params::{ToneParams, WahSel};
use crate::widgets::{CONTROL_H, KNOB_SIZE, card_at, knob, led_toggle, pill, segmented, theme};
use crate::{Shared, Task, ToneTinkerer};
use egui::{Color32, CornerRadius, Rect, RichText, Ui, pos2, vec2};
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::*;
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, NiceEguiApp, RepaintNotifier, create_egui_editor,
};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tonetinkerer_dsp::chain::ChainParams;
use tonetinkerer_dsp::presets::{Genre, PRESETS, Preset};

pub const WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(1120.0, 700.0);
const GAP: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Play,
    Advanced,
}

pub struct ToneEditor {
    pub(crate) params: Arc<ToneParams>,
    pub(crate) shared: Arc<Shared>,
    executor: AsyncExecutor<ToneTinkerer>,
    gui: Option<GuiContext>,
    pub(crate) in_peak: f32,
    pub(crate) out_peak: [f32; 2],
    pub(crate) view: View,
    pub(crate) genre: Genre,
    /// Guitar setup in progress: (start time, peak so far).
    pub(crate) setup: Option<(Instant, f32)>,
    pub(crate) setup_msg: Option<String>,
    background: Option<egui::TextureHandle>,
}

pub fn create(
    params: Arc<ToneParams>,
    shared: Arc<Shared>,
    state: Arc<EguiEditorState>,
    repaint: RepaintNotifier,
    executor: AsyncExecutor<ToneTinkerer>,
) -> Option<EguiEditor<ToneEditor>> {
    let genre = PRESETS
        .iter()
        .find(|p| p.name == *params.preset_name.read())
        .map_or(Genre::Clean, |p| p.genre);
    let app = ToneEditor {
        params,
        shared,
        executor,
        gui: None,
        in_peak: 0.0,
        out_peak: [0.0; 2],
        view: View::Play,
        genre,
        setup: None,
        setup_msg: None,
        background: None,
    };
    create_egui_editor(
        state,
        repaint,
        EguiNiceSettings::new().with_resize_hint(ResizeHint::non_resizable()),
        app,
    )
}

fn fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut f = FontDefinitions::default();
    let add = |f: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data
            .insert(name.to_owned(), std::sync::Arc::new(FontData::from_static(bytes)));
    };
    add(&mut f, "inter", include_bytes!("../../../assets/fonts/Inter-Regular.ttf"));
    add(&mut f, "inter-medium", include_bytes!("../../../assets/fonts/Inter-Medium.ttf"));
    add(&mut f, "inter-semibold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"));
    // Inter first everywhere (numbers too: no monospace look); egui's own
    // fonts stay behind it for symbols Inter lacks.
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        f.families.get_mut(&fam).unwrap().insert(0, "inter".to_owned());
    }
    let fallback = f.families[&FontFamily::Proportional].clone();
    for (fam, first) in [(theme::MEDIUM, "inter-medium"), (theme::SEMIBOLD, "inter-semibold")] {
        let mut list = vec![first.to_owned()];
        list.extend(fallback.iter().cloned());
        f.families.insert(FontFamily::Name(fam.into()), list);
    }
    ctx.set_fonts(f);
}

fn style(ctx: &egui::Context) {
    fonts(ctx);
    let mut v = egui::Visuals::dark();
    v.panel_fill = theme::BG;
    v.window_fill = theme::PANEL;
    v.extreme_bg_color = Color32::from_rgb(20, 21, 27);
    // Standard widgets match the raised keys in `widgets::key`.
    let radius = CornerRadius::same(6);
    for (w, fill) in [
        (&mut v.widgets.inactive, Color32::from_rgb(40, 42, 62)),
        (&mut v.widgets.hovered, Color32::from_rgb(50, 52, 76)),
        (&mut v.widgets.active, Color32::from_rgb(30, 32, 48)),
        (&mut v.widgets.open, Color32::from_rgb(30, 32, 48)),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = egui::Stroke::new(1.0, Color32::from_black_alpha(160));
        w.corner_radius = radius;
        w.expansion = 0.0;
    }
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
    v.window_corner_radius = CornerRadius::same(12);
    v.window_stroke = egui::Stroke::new(1.0, Color32::from_white_alpha(14));
    v.menu_corner_radius = CornerRadius::same(10);
    v.selection.bg_fill = theme::ACCENT.gamma_multiply(0.45);
    v.selection.stroke = egui::Stroke::new(1.0, theme::ACCENT);
    v.override_text_color = Some(theme::TEXT);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        use egui::TextStyle;
        s.text_styles.insert(TextStyle::Button, theme::medium(12.5));
        s.text_styles.insert(TextStyle::Body, theme::regular(13.0));
        s.text_styles.insert(TextStyle::Small, theme::regular(11.0));
        s.spacing.button_padding = vec2(12.0, 5.0);
        s.spacing.interact_size.y = CONTROL_H;
    });
}

/// Compact host-aware combo box for an EnumParam.
pub(crate) fn enum_combo<T: Enum + PartialEq + Copy + 'static>(
    ui: &mut Ui,
    setter: &ParamSetter,
    param: &EnumParam<T>,
    width: f32,
) {
    let cur = param.value().to_index();
    egui::ComboBox::from_id_salt(param.name())
        .width(width)
        .selected_text(T::variants()[cur])
        .show_ui(ui, |ui| {
            for (i, name) in T::variants().iter().enumerate() {
                if ui.selectable_label(i == cur, *name).clicked() && i != cur {
                    setter.begin_set_parameter(param);
                    setter.set_parameter(param, T::from_index(i));
                    setter.end_set_parameter(param);
                }
            }
        });
}

/// Does the sound's detailed state still match the factory sound? Macros
/// are not edits (they are the intended way to adjust a sound); the guitar /
/// input / output calibration is not part of a sound either.
fn matches(params: &ToneParams, preset: &Preset) -> bool {
    let mut cur = ChainParams::default();
    params.base_params(None, &mut cur);
    let mut p = (preset.params)();
    p.input_mode = cur.input_mode;
    p.guitar = cur.guitar;
    p.input_gain_db = cur.input_gain_db;
    p.output_db = cur.output_db;
    if params.delay_sync.value() != crate::params::DelaySync::Free {
        return false;
    }
    cur.tone_fields()
        .iter()
        .zip(p.tone_fields().iter())
        .all(|(a, b)| (a - b).abs() <= 0.002 * b.abs().max(1.0))
}

impl ToneEditor {
    pub(crate) fn is_edited(&self, preset: &Preset) -> bool {
        !matches(&self.params, preset)
    }

    pub(crate) fn update_meters(&mut self) {
        let decay = 0.86;
        self.in_peak = self
            .shared
            .input_peak
            .swap(0.0, Ordering::Relaxed)
            .max(self.in_peak * decay);
        self.out_peak[0] = self
            .shared
            .output_peak_l
            .swap(0.0, Ordering::Relaxed)
            .max(self.out_peak[0] * decay);
        self.out_peak[1] = self
            .shared
            .output_peak_r
            .swap(0.0, Ordering::Relaxed)
            .max(self.out_peak[1] * decay);
    }

    /// Sound selector in the middle of the shared top bar.
    fn sound_browser(&mut self, ui: &mut Ui, setter: &ParamSetter, bar: egui::Rect) {
        let rect = bar;
        // Sound browser (grouped by genre).
        let name = self.params.preset_name.read().clone();
        let idx = PRESETS.iter().position(|q| q.name == name);
        let edited = idx.is_some_and(|i| self.is_edited(&PRESETS[i]));
        let browser =
            egui::Rect::from_center_size(rect.center() + vec2(-20.0, 0.0), vec2(390.0, 34.0));
        let mut b = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(browser)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        b.spacing_mut().item_spacing.x = 6.0;
        let n = PRESETS.len();
        if b.add_sized(
            [30.0, 28.0],
            egui::Button::new(RichText::new("◀").size(13.0)),
        )
        .on_hover_text("Previous sound")
        .clicked()
        {
            let i = idx.map_or(0, |i| (i + n - 1) % n);
            crate::play::apply_sound(setter, &self.params, &PRESETS[i]);
        }
        let label = format!("{}{}", name, if edited { "  •  edited" } else { "" });
        egui::ComboBox::from_id_salt("preset")
            .width(300.0)
            .height(560.0)
            .selected_text(RichText::new(label).size(14.0).strong())
            .show_ui(&mut b, |ui| {
                for g in Genre::ALL {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(g.label().to_uppercase())
                            .size(10.0)
                            .color(theme::ACCENT),
                    );
                    for preset in PRESETS.iter().filter(|p| p.genre == g) {
                        let text = match preset.inspired_by {
                            Some(i) => format!("{}   ·  {}", preset.name, i),
                            None => preset.name.to_owned(),
                        };
                        if ui
                            .selectable_label(preset.name == name, RichText::new(text).size(13.0))
                            .on_hover_text(preset.description)
                            .clicked()
                        {
                            crate::play::apply_sound(setter, &self.params, preset);
                        }
                    }
                }
            });
        if b.add_sized(
            [30.0, 28.0],
            egui::Button::new(RichText::new("▶").size(13.0)),
        )
        .on_hover_text("Next sound")
        .clicked()
        {
            let i = idx.map_or(0, |i| (i + 1) % n);
            crate::play::apply_sound(setter, &self.params, &PRESETS[i]);
        }

    }

    fn advanced(&mut self, ui: &mut Ui, setter: &ParamSetter, top: f32) {
        let p = self.params.clone();
        let left = ui.max_rect().left() + 20.0;
        let row = |i: usize| top + i as f32 * (ROW_H + GAP);
        let cards = |y: f32, widths: &[f32]| -> Vec<Rect> {
            let mut x = left;
            widths
                .iter()
                .map(|w| {
                    let r = Rect::from_min_size(pos2(x, y), vec2(*w, ROW_H));
                    x += w + GAP;
                    r
                })
                .collect()
        };

        // ---------------------------------------------------------- row 1
        let r = cards(row(0), &[230.0, 250.0, 300.0, 264.0]);
        let (_, dyn_header, body) = card_at(ui, r[0], "DYNAMICS", theme::GATE, None);
        let col_w = body.width() / 2.0;
        let red = self.shared.gate_reduction_db.load(Ordering::Relaxed);
        let hiss = self.shared.noise_floor_db.load(Ordering::Relaxed);
        let state = if !p.gate_on.value() {
            "off"
        } else if red > -3.0 {
            "open"
        } else {
            "closed"
        };
        let gate_txt = format!("gate {state}");
        // The background hiss the gate measured (it closes just above it).
        if p.gate_on.value() && hiss > -110.0 {
            ui.painter().text(
                dyn_header.right_center(),
                egui::Align2::RIGHT_CENTER,
                format!("hiss {hiss:.0} dB"),
                theme::regular(10.5),
                theme::MUTED,
            );
        }
        let gr = if p.comp_on.value() {
            self.shared.comp_reduction_db.load(Ordering::Relaxed)
        } else {
            0.0
        };
        let comp_txt = if gr < 0.5 { "comp 0 dB".to_owned() } else { format!("comp -{gr:.0} dB") };
        for (i, (on, accent, txt)) in [
            (&p.gate_on, theme::GATE, gate_txt),
            (&p.comp_on, theme::SUSTAIN, comp_txt),
        ]
        .into_iter()
        .enumerate()
        {
            let col = Rect::from_min_size(
                pos2(body.left() + i as f32 * col_w, body.top()),
                vec2(col_w, CONTROL_H),
            );
            let mut c = at(ui, col, egui::Layout::left_to_right(egui::Align::Center));
            c.spacing_mut().item_spacing.x = 4.0;
            led_toggle(&mut c, setter, on, accent);
            c.label(RichText::new(txt).size(11.0).color(theme::MUTED));
        }
        let kr = knob_row(body);
        let mut k = at(ui, Rect::from_min_size(kr.min, vec2(col_w, kr.height())), row_layout());
        knob(&mut k, setter, &p.gate_threshold, "Gate", theme::GATE, p.gate_on.value());
        let mut k = at(
            ui,
            Rect::from_min_size(kr.min + vec2(col_w, 0.0), vec2(col_w, kr.height())),
            row_layout(),
        );
        knob(&mut k, setter, &p.sustain, "Compressor", theme::SUSTAIN, p.comp_on.value());

        let (_, _, body) = card_at(ui, r[1], "WAH", theme::WAH, None);
        let on = p.wah_mode.value() != WahSel::Off;
        let mut c = at(ui, controls_row(body), row_layout());
        segmented(&mut c, setter, &p.wah_mode, theme::WAH, body.width());
        let mut k = knobs(ui, body);
        knob(&mut k, setter, &p.wah_position, "Position", theme::WAH, on)
            .on_hover_text("Manual: the pedal position (map to an expression pedal). Auto: the resting position.");
        knob(&mut k, setter, &p.wah_sensitivity, "Sensitivity", theme::WAH, p.wah_mode.value() == WahSel::Auto)
            .on_hover_text("Auto mode: how far your picking opens the wah.");

        let (on, _, body) = card_at(ui, r[2], "OVERDRIVE", theme::DRIVE, Some((setter, &p.drive_on)));
        let mut c = at(ui, controls_row(body), row_layout());
        pill(&mut c, setter, &p.drive_fat, "Fat", theme::DRIVE)
            .on_hover_text("Keeps the low mids: lowers the clipping stage's bass cut (720 Hz -> 250 Hz).");
        let mut k = knobs(ui, body);
        knob(&mut k, setter, &p.drive, "Drive", theme::DRIVE, on);
        knob(&mut k, setter, &p.drive_tone, "Tone", theme::DRIVE, on);
        knob(&mut k, setter, &p.drive_level, "Level", theme::DRIVE, on);

        let (_, io_header, body) = card_at(ui, r[3], "IN / OUT", theme::OUTPUT, None);
        // What the automatic input level does (autolevel.rs).
        let auto = self.shared.auto_level_db.load(Ordering::Relaxed);
        let learned = self.shared.level_learned.load(Ordering::Relaxed);
        ui.painter().text(
            io_header.right_center(),
            egui::Align2::RIGHT_CENTER,
            if learned { format!("auto {auto:+.0} dB") } else { format!("auto {auto:+.0} dB…") },
            theme::regular(10.5),
            theme::MUTED,
        );
        let mut c = at(ui, controls_row(body), row_layout());
        c.spacing_mut().item_spacing.x = 8.0;
        enum_combo(&mut c, setter, &p.input_sel, 110.0);
        pill(&mut c, setter, &p.limiter, "Limiter", theme::ACCENT)
            .on_hover_text("Soft ceiling at 0 dBFS.");
        let mut k = knobs(ui, body);
        knob(&mut k, setter, &p.input_gain, "Input Trim", theme::OUTPUT, true);
        knob(&mut k, setter, &p.output, "Output", theme::OUTPUT, true);

        // ---------------------------------------------------------- row 2
        let r = cards(row(1), &[700.0, 368.0]);
        let (_, header, body) = card_at(ui, r[0], "AMPLIFIER  ·  NEURAL (NAM)", theme::ACCENT, None);
        let mut h = at(ui, header.expand2(vec2(0.0, 2.0)), egui::Layout::right_to_left(egui::Align::Center));
        h.spacing_mut().item_spacing.x = 10.0;
        if h
            .button("Load .nam…")
            .on_hover_text("Load any Neural Amp Modeler capture into the Custom slot")
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new().add_filter("NAM model", &["nam"]).pick_file() {
                *p.custom_amp_path.write() = Some(path.to_string_lossy().into_owned());
                self.executor.execute_background(Task::LoadAmp);
                setter.begin_set_parameter(&p.amp_model);
                setter.set_parameter(&p.amp_model, crate::params::AmpSel::Custom);
                setter.end_set_parameter(&p.amp_model);
            }
        }
        let st = self.shared.amp_status.lock().clone();
        if let Some(e) = st.error {
            h.label(RichText::new(format!("⚠ {e}")).size(11.0).color(theme::RED));
        } else if let Some(n) = st.name {
            h.label(RichText::new(format!("Custom: {n}")).size(11.0).color(theme::MUTED));
        }
        let mut c = at(ui, controls_row(body), row_layout());
        segmented(&mut c, setter, &p.amp_model, theme::ACCENT, 430.0);
        let mut k = knobs(ui, body);
        k.spacing_mut().item_spacing.x = 12.0;
        knob(&mut k, setter, &p.amp_gain, "Gain", theme::ACCENT, true);
        knob(&mut k, setter, &p.bass, "Bass", theme::ACCENT, true);
        knob(&mut k, setter, &p.mid, "Middle", theme::ACCENT, true);
        knob(&mut k, setter, &p.mid_freq, "Mid Freq", theme::ACCENT, true);
        knob(&mut k, setter, &p.treble, "Treble", theme::ACCENT, true);
        knob(&mut k, setter, &p.presence, "Presence", theme::ACCENT, true);
        knob(&mut k, setter, &p.body, "Body", theme::ACCENT, true);
        knob(&mut k, setter, &p.amp_level, "Master", theme::ACCENT, true);

        let (on, header, body) = card_at(ui, r[1], "CABINET", theme::CAB, Some((setter, &p.cab_on)));
        let mut h = at(ui, header.expand2(vec2(0.0, 2.0)), egui::Layout::right_to_left(egui::Align::Center));
        h.spacing_mut().item_spacing.x = 10.0;
        if h
            .button("Load IR…")
            .on_hover_text("Load a cabinet impulse response (.wav) into the Custom slot")
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new().add_filter("Impulse response", &["wav"]).pick_file() {
                *p.custom_ir_path.write() = Some(path.to_string_lossy().into_owned());
                self.executor.execute_background(Task::LoadIr);
                setter.begin_set_parameter(&p.cab_ir);
                setter.set_parameter(&p.cab_ir, crate::params::CabSel::Custom);
                setter.end_set_parameter(&p.cab_ir);
            }
        }
        let st = self.shared.ir_status.lock().clone();
        if let Some(e) = st.error {
            h.label(RichText::new(format!("⚠ {e}")).size(11.0).color(theme::RED));
        } else if let Some(n) = st.name {
            h.label(RichText::new(n).size(11.0).color(theme::MUTED));
        }
        let mut c = at(ui, controls_row(body), row_layout());
        segmented(&mut c, setter, &p.cab_ir, theme::CAB, body.width());
        let mut k = knobs(ui, body);
        knob(&mut k, setter, &p.cab_low_cut, "Low Cut", theme::CAB, on);
        knob(&mut k, setter, &p.cab_high_cut, "High Cut", theme::CAB, on);

        // ---------------------------------------------------------- row 3
        let r = cards(row(2), &[240.0, 486.0, 330.0]);
        let (on, _, body) = card_at(ui, r[0], "CHORUS", theme::CHORUS, Some((setter, &p.chorus_on)));
        let mut k = knobs(ui, body);
        knob(&mut k, setter, &p.chorus_rate, "Rate", theme::CHORUS, on);
        knob(&mut k, setter, &p.chorus_depth, "Depth", theme::CHORUS, on);
        knob(&mut k, setter, &p.chorus_mix, "Mix", theme::CHORUS, on);

        let (on, _, body) = card_at(ui, r[1], "STEREO DELAY", theme::DELAY, Some((setter, &p.delay_on)));
        let mut c = at(ui, controls_row(body), row_layout());
        c.spacing_mut().item_spacing.x = 8.0;
        c.label(RichText::new("Sync").size(11.5).color(theme::MUTED));
        enum_combo(&mut c, setter, &p.delay_sync, 120.0);
        pill(&mut c, setter, &p.delay_ping_pong, "Ping-Pong", theme::DELAY);
        let synced = p.delay_sync.value() != crate::params::DelaySync::Free;
        let mut k = knobs(ui, body);
        knob(&mut k, setter, &p.delay_time, "Time", theme::DELAY, on && !synced);
        knob(&mut k, setter, &p.delay_feedback, "Feedback", theme::DELAY, on);
        knob(&mut k, setter, &p.delay_mix, "Mix", theme::DELAY, on);
        knob(&mut k, setter, &p.delay_tone, "Tone", theme::DELAY, on);
        knob(&mut k, setter, &p.delay_duck, "Duck", theme::DELAY, on)
            .on_hover_text("Repeats get quieter while you play and bloom in the gaps.");
        knob(&mut k, setter, &p.delay_mod, "Mod", theme::DELAY, on);

        let (on, _, body) = card_at(ui, r[2], "REVERB", theme::REVERB, Some((setter, &p.reverb_on)));
        let mut k = knobs(ui, body);
        knob(&mut k, setter, &p.reverb_decay, "Decay", theme::REVERB, on);
        knob(&mut k, setter, &p.reverb_predelay, "Pre-Delay", theme::REVERB, on);
        knob(&mut k, setter, &p.reverb_damping, "Damping", theme::REVERB, on);
        knob(&mut k, setter, &p.reverb_mix, "Mix", theme::REVERB, on);

        // --------------------------------------------------------- footer
        let sr = self.shared.sample_rate.load(Ordering::Relaxed);
        let lat = self.shared.latency.load(Ordering::Relaxed);
        let ms = lat as f32 / sr * 1000.0;
        let tip = if (sr - 48_000.0).abs() > 1.0 {
            "  ·  tip: run your project at 48 kHz for the lowest latency"
        } else {
            ""
        };
        ui.painter().text(
            pos2(left + 4.0, row(3) + 2.0),
            egui::Align2::LEFT_TOP,
            format!(
                "{:.1} kHz  ·  latency {lat} samples ({ms:.2} ms){tip}  ·  Bundled amps are NAM captures of ToneTinkerer's own reference circuits; load any .nam capture via Custom.",
                sr / 1000.0
            ),
            theme::regular(10.5),
            theme::FAINT,
        );
    }
}

const ROW_H: f32 = 186.0;

fn at(ui: &mut Ui, rect: Rect, layout: egui::Layout) -> Ui {
    ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout))
}

fn row_layout() -> egui::Layout {
    egui::Layout::left_to_right(egui::Align::Center)
}

/// The controls line at the top of a card body (segmented, pills, combos).
fn controls_row(body: Rect) -> Rect {
    Rect::from_min_size(body.min, vec2(body.width(), CONTROL_H))
}

/// The knob line, pinned to the bottom of a card body so knobs line up
/// across every card in a row.
fn knob_row(body: Rect) -> Rect {
    Rect::from_min_max(pos2(body.left(), body.bottom() - KNOB_SIZE.y), body.max)
}

fn knobs(ui: &mut Ui, body: Rect) -> Ui {
    let mut k = at(ui, knob_row(body), egui::Layout::left_to_right(egui::Align::Min));
    k.spacing_mut().item_spacing.x = 6.0;
    k
}

impl NiceEguiApp for ToneEditor {
    fn build(
        &mut self,
        egui_ctx: egui::Context,
        gui: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        self.init(&egui_ctx);
        self.gui = Some(gui);
        Ok(())
    }

    fn editor_closed(&mut self) {
        self.gui = None;
        self.background = None;
        self.setup = None;
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut nice_plug_egui::Frame) {
        self.shared.empty_trash();
        let Some(gui) = self.gui.clone() else { return };
        self.draw(ui, &gui.param_setter());
    }
}

impl ToneEditor {
    /// An editor without a host window (offscreen snapshot tests).
    #[cfg(test)]
    pub(crate) fn headless(params: Arc<ToneParams>, shared: Arc<Shared>, view: View, genre: Genre) -> Self {
        Self {
            params,
            shared,
            executor: AsyncExecutor::new(Arc::new(|_| {}), Arc::new(|_| {})),
            gui: None,
            in_peak: 0.25,
            out_peak: [0.4, 0.38],
            view,
            genre,
            setup: None,
            setup_msg: None,
            background: None,
        }
    }

    /// Install the theme and skin on a context (the host path does this in `build`).
    pub(crate) fn init(&mut self, ctx: &egui::Context) {
        style(ctx);
        self.background = crate::skin::load(ctx);
    }

    pub(crate) fn draw(&mut self, ui: &mut Ui, setter: &ParamSetter) {
        let rect = ui.max_rect();
        crate::skin::paint(ui, rect, self.background.as_ref());
        match self.view {
            View::Play => crate::play::ui(self, ui, setter),
            View::Advanced => {
                let full = rect.shrink2(vec2(20.0, 16.0));
                let bar = Rect::from_min_size(full.min, vec2(full.width(), 44.0));
                crate::play::top_bar(self, ui, bar);
                self.sound_browser(ui, setter, bar);
                self.advanced(ui, setter, bar.bottom() + 12.0);
            }
        }
        if self.in_peak > 1e-6 || self.out_peak[0] > 1e-6 {
            ui.ctx().request_repaint();
        }
    }
}
