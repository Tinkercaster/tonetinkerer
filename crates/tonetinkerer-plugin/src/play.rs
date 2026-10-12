//! The Play screen: pick a sound, play, and only touch a few plain-language
//! controls if something is missing.

use crate::editor::{ToneEditor, View};
use crate::params::{ToneParams, apply_chain_params};
use crate::widgets::{
    display, etched, fix_button, key, led, lit_bar, logo_mark, macro_knob, meter,
    segmented_labeled, theme, tracked, well,
};
use egui::{
    Align2, Color32, CornerRadius, Rect, RichText, Sense, Ui, pos2,
    vec2,
};
use nice_plug::prelude::*;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tonetinkerer_dsp::presets::{Genre, PRESETS, Preset};

fn panel(ui: &Ui, rect: Rect) {
    crate::widgets::panel(ui, rect, 12);
}

/// One line of text cut to `max_w` with an ellipsis.
fn text_elided(
    ui: &Ui,
    pos: egui::Pos2,
    text: &str,
    font: egui::FontId,
    color: Color32,
    max_w: f32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(pos, galley, color);
}

/// Small, quiet section heading.
fn heading(ui: &Ui, pos: egui::Pos2, text: &str) {
    tracked(ui, pos, Align2::LEFT_TOP, text, theme::semibold(10.0), theme::FAINT, 1.8);
}

/// Text-only button (hover tints it). Returns the response.
fn link(ui: &mut Ui, rect: Rect, id: &str, text: &str, align: Align2) -> egui::Response {
    let r = ui.interact(rect, ui.id().with(id), Sense::click());
    let col = if r.hovered() { theme::ACCENT } else { theme::MUTED };
    let pos = match align {
        Align2::RIGHT_CENTER => rect.right_center(),
        Align2::CENTER_CENTER => rect.center(),
        _ => rect.left_center(),
    };
    ui.painter().text(pos, align, text, theme::medium(12.0), col);
    r
}

fn child(ui: &mut Ui, rect: Rect, layout: egui::Layout) -> Ui {
    ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout))
}

/// Nudge a macro by `delta` (one host gesture).
fn nudge(setter: &ParamSetter, p: &FloatParam, delta: f32) {
    setter.begin_set_parameter(p);
    setter.set_parameter(p, (p.value() + delta).clamp(-1.0, 1.0));
    setter.end_set_parameter(p);
}

pub fn apply_sound(setter: &ParamSetter, params: &ToneParams, preset: &Preset) {
    apply_chain_params(setter, params, &(preset.params)());
    *params.preset_name.write() = preset.name.to_owned();
}

/// Shared top bar for both views: wordmark left, meters and the view switch
/// right. The switch sits in the same place on both screens.
pub fn top_bar(ed: &mut ToneEditor, ui: &mut Ui, bar: Rect) {
    let p = ui.painter().clone();
    let c = bar.left_center() + vec2(16.0, 0.0);
    logo_mark(&p, c, 14.0);
    tracked(
        ui,
        c + vec2(28.0, -6.0),
        Align2::LEFT_CENTER,
        "TONETINKERER",
        theme::semibold(15.0),
        theme::TEXT,
        2.4,
    );
    tracked(
        ui,
        c + vec2(28.5, 11.0),
        Align2::LEFT_CENTER,
        "BY GUITARWIRING",
        theme::medium(8.5),
        theme::FAINT,
        2.2,
    );
    ed.update_meters();
    let (label, target, tip) = match ed.view {
        View::Play => (
            "Advanced  ›",
            View::Advanced,
            "Every detailed control: pedals, amp, cabinet, effects",
        ),
        View::Advanced => ("‹  Simple", View::Play, "Back to the simple screen"),
    };
    let sw = Rect::from_min_size(bar.right_center() - vec2(104.0, 15.0), vec2(104.0, 30.0));
    let r = ui.interact(sw, ui.id().with("view_switch"), Sense::click());
    key(&p, sw, 6, r.hovered(), r.is_pointer_button_down_on());
    p.text(sw.center(), Align2::CENTER_CENTER, label, theme::medium(12.5), theme::TEXT);
    if r.on_hover_text(tip).clicked() {
        ed.view = target;
    }
    let mrect = Rect::from_min_size(sw.left_top() + vec2(-96.0, -8.0), vec2(80.0, 48.0));
    let mut m = child(ui, mrect, egui::Layout::left_to_right(egui::Align::Min));
    m.spacing_mut().item_spacing.x = 12.0;
    meter(&mut m, "IN", &[ed.in_peak], 30.0, Some((-18.0, -6.0))).on_hover_text(
        "Your guitar level. 'Set up my guitar' puts hard playing in the shaded zone.",
    );
    meter(&mut m, "OUT", &ed.out_peak, 30.0, None).on_hover_text("Output level");
}

pub fn ui(ed: &mut ToneEditor, ui: &mut Ui, setter: &ParamSetter) {
    let full = ui.max_rect().shrink2(vec2(20.0, 16.0));
    let params = ed.params.clone();
    let current = params.preset_name.read().clone();
    let current_preset = PRESETS.iter().find(|p| p.name == current);
    let gap = 14.0;

    // ------------------------------------------------------------ top bar
    let bar = Rect::from_min_size(full.min, vec2(full.width(), 44.0));
    top_bar(ed, ui, bar);

    // ------------------------------------------------------- sidebar
    let top = bar.bottom() + gap;
    let side = Rect::from_min_max(pos2(full.left(), top), pos2(full.left() + 300.0, full.bottom()));
    panel(ui, side);
    let inner = side.shrink2(vec2(16.0, 16.0));
    heading(ui, inner.left_top(), "SOUNDS");
    // Genres: one recessed slot of 3 x 2 keys; the chosen one is raised and lit.
    let chips_top = inner.top() + 24.0;
    let tab_h = 30.0;
    let grid = Rect::from_min_size(
        pos2(inner.left(), chips_top),
        vec2(inner.width(), 2.0 * tab_h + 4.0),
    );
    well(ui.painter(), grid, 7);
    let tab_w = (grid.width() - 4.0) / 3.0;
    for (i, g) in Genre::ALL.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(
                grid.left() + 2.0 + (i % 3) as f32 * tab_w,
                grid.top() + 2.0 + (i / 3) as f32 * tab_h,
            ),
            vec2(tab_w, tab_h),
        )
        .shrink(1.0);
        let resp = ui.interact(r, ui.id().with(("genre", i)), Sense::click());
        let sel = ed.genre == *g;
        if sel {
            key(ui.painter(), r, 5, resp.hovered(), false);
            lit_bar(ui.painter(), r, theme::ACCENT);
        }
        ui.painter().text(
            r.center() - vec2(0.0, if sel { 2.0 } else { 0.0 }),
            Align2::CENTER_CENTER,
            g.label(),
            theme::medium(12.0),
            if sel || resp.hovered() { theme::TEXT } else { theme::MUTED },
        );
        if resp.clicked() {
            ed.genre = *g;
        }
    }
    // Guitar setup lives at the bottom of the sidebar.
    let setup_h = 142.0;
    let setup_rect = Rect::from_min_max(pos2(inner.left(), inner.bottom() - setup_h), inner.max);
    let list = Rect::from_min_max(
        pos2(inner.left() - 6.0, grid.bottom() + 10.0),
        pos2(inner.right() + 6.0, setup_rect.top() - 12.0),
    );
    let mut l = child(ui, list, egui::Layout::top_down(egui::Align::Min));
    l.spacing_mut().item_spacing.y = 2.0;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut l, |ui| {
            for preset in PRESETS.iter().filter(|p| p.genre == ed.genre) {
                let (r, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
                let sel = preset.name == current;
                let pnt = ui.painter();
                if sel {
                    key(pnt, r.shrink(1.0), 8, false, false);
                    led(pnt, pos2(r.right() - 16.0, r.center().y), true, theme::ACCENT);
                } else if resp.hovered() {
                    pnt.rect_filled(r, CornerRadius::same(8), Color32::from_white_alpha(7));
                }
                pnt.text(
                    r.left_top() + vec2(14.0, 9.0),
                    Align2::LEFT_TOP,
                    preset.name,
                    theme::medium(14.0),
                    theme::TEXT,
                );
                let sub = preset
                    .inspired_by
                    .map(|i| format!("Inspired by {i}"))
                    .unwrap_or_else(|| "Original".to_owned());
                text_elided(
                    ui,
                    r.left_top() + vec2(14.0, 29.0),
                    &sub,
                    theme::regular(11.5),
                    theme::MUTED,
                    r.width() - 44.0,
                );
                if resp.on_hover_text(preset.description).clicked() {
                    apply_sound(setter, &params, preset);
                }
            }
        });
    etched(
        ui.painter(),
        pos2(side.left() + 1.0, setup_rect.top() - 6.0),
        pos2(side.right() - 1.0, setup_rect.top() - 6.0),
    );
    heading(ui, setup_rect.left_top() + vec2(0.0, 6.0), "MY GUITAR");
    // Label over a full-width selector, then one full-width action. (No
    // pickup selector: the input level is set automatically, autolevel.rs.)
    let w = inner.width();
    let row = |i: usize| setup_rect.top() + 30.0 + i as f32 * 54.0;
    for (i, text) in ["Input"].iter().enumerate() {
        tracked(
            ui,
            pos2(inner.left(), row(i) + 1.0),
            Align2::LEFT_TOP,
            &text.to_uppercase(),
            theme::medium(9.5),
            theme::MUTED,
            1.4,
        );
    }
    let seg = |i: usize| Rect::from_min_size(pos2(inner.left(), row(i) + 18.0), vec2(w, 28.0));
    let mut r1 = child(ui, seg(0), egui::Layout::left_to_right(egui::Align::Min));
    segmented_labeled(&mut r1, setter, &params.input_sel, &["Auto", "Input 1", "Input 2"], theme::ACCENT, w)
        .map(|_| ());
    let action = Rect::from_min_size(pos2(inner.left(), row(1) + 4.0), vec2(inner.width(), 36.0));
    setup_action(ed, ui, action);

    // ------------------------------------------------------ hero + knobs
    let right = Rect::from_min_max(pos2(side.right() + gap, top), full.max);
    let main = Rect::from_min_size(right.min, vec2(right.width(), 330.0));
    panel(ui, main);
    let mi = main.shrink2(vec2(26.0, 22.0));
    display(ui.painter(), Rect::from_min_size(mi.min - vec2(12.0, 8.0), vec2(mi.width() + 24.0, 132.0)), 8);
    if let Some(pr) = current_preset {
        let edited = ed.is_edited(pr);
        let p = ui.painter();
        tracked(
            ui,
            mi.left_top(),
            Align2::LEFT_TOP,
            &pr.genre.label().to_uppercase(),
            theme::semibold(10.5),
            theme::ACCENT,
            2.2,
        );
        let title = p.text(
            mi.left_top() + vec2(0.0, 18.0),
            Align2::LEFT_TOP,
            pr.name,
            theme::semibold(30.0),
            theme::TEXT,
        );
        if edited {
            let tag = Rect::from_min_size(
                pos2(title.right() + 12.0, title.center().y - 11.0),
                vec2(62.0, 22.0),
            );
            p.rect_filled(tag, CornerRadius::same(11), Color32::from_white_alpha(12));
            p.text(tag.center(), Align2::CENTER_CENTER, "Edited", theme::medium(11.0), theme::MUTED);
        }
        let sub = pr
            .inspired_by
            .map(|i| format!("Inspired by {i}"))
            .unwrap_or_else(|| "Original sound".to_owned());
        p.text(
            mi.left_top() + vec2(0.0, 58.0),
            Align2::LEFT_TOP,
            sub,
            theme::regular(13.0),
            theme::MUTED,
        );
        let mut d = child(
            ui,
            Rect::from_min_size(mi.left_top() + vec2(0.0, 82.0), vec2(mi.width().min(640.0), 40.0)),
            egui::Layout::top_down(egui::Align::Min),
        );
        d.label(RichText::new(pr.description).size(13.0).color(theme::FAINT));
    }
    // The main knobs sit on a raised plate, like a hardware front panel.
    key(
        ui.painter(),
        Rect::from_min_max(pos2(mi.left() - 12.0, mi.top() + 136.0), pos2(mi.right() + 12.0, mi.bottom() + 8.0)),
        8,
        false,
        false,
    );
    let mut k = child(
        ui,
        Rect::from_min_max(pos2(mi.left() - 10.0, mi.top() + 152.0), pos2(mi.right() + 10.0, mi.bottom())),
        egui::Layout::left_to_right(egui::Align::Min),
    );
    k.spacing_mut().item_spacing.x = 0.0;
    let knob_list: [(&FloatParam, &str, &str, &str); 7] = [
        (&params.m_gain, "Gain", "Clean", "Fierce"),
        (&params.m_body, "Body", "Thin", "Thick"),
        (&params.m_bright, "Bright", "Dark", "Bright"),
        (&params.m_mids, "Mids", "Scoop", "Push"),
        (&params.m_sustain, "Sustain", "Short", "Long"),
        (&params.m_space, "Space", "Dry", "Huge"),
        (&params.m_volume, "Volume", "Quiet", "Loud"),
    ];
    let col_w = k.available_width() / knob_list.len() as f32;
    for (i, (param, label, lo, hi)) in knob_list.into_iter().enumerate() {
        macro_knob(&mut k, setter, param, label, lo, hi, theme::MACRO[i], col_w);
    }

    // ------------------------------------------------------- quick fixes
    let fixes = Rect::from_min_max(pos2(right.left(), main.bottom() + gap), right.max);
    panel(ui, fixes);
    let fi = fixes.shrink2(vec2(26.0, 20.0));
    ui.painter().text(fi.left_top(), Align2::LEFT_TOP, "Quick fixes", theme::semibold(15.0), theme::TEXT);
    ui.painter().text(
        fi.left_top() + vec2(0.0, 22.0),
        Align2::LEFT_TOP,
        "Something missing? Tap what you hear.",
        theme::regular(12.0),
        theme::FAINT,
    );
    let reset = Rect::from_min_size(fi.right_top() - vec2(200.0, 0.0), vec2(200.0, 20.0));
    if link(ui, reset, "reset_macros", "↺  Reset to the designed sound", Align2::RIGHT_CENTER)
        .clicked()
    {
        for m in [
            &params.m_gain,
            &params.m_body,
            &params.m_bright,
            &params.m_mids,
            &params.m_sustain,
            &params.m_space,
        ] {
            setter.begin_set_parameter(m);
            setter.set_parameter(m, 0.0);
            setter.end_set_parameter(m);
        }
    }
    let mut f = child(
        ui,
        Rect::from_min_max(fi.left_top() + vec2(0.0, 52.0), fi.max),
        egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
    );
    f.spacing_mut().item_spacing = vec2(8.0, 8.0);
    let step = 0.25;
    let a = theme::ACCENT;
    if fix_button(&mut f, "Too thin", a) {
        nudge(setter, &params.m_body, step);
    }
    if fix_button(&mut f, "Too muddy", a) {
        nudge(setter, &params.m_body, -step);
        nudge(setter, &params.m_bright, step * 0.5);
    }
    if fix_button(&mut f, "Too harsh", a) {
        nudge(setter, &params.m_bright, -step);
    }
    if fix_button(&mut f, "Too dull", a) {
        nudge(setter, &params.m_bright, step);
    }
    if fix_button(&mut f, "Too boxy", a) {
        nudge(setter, &params.m_mids, -step);
    }
    if fix_button(&mut f, "Lost in the mix", a) {
        nudge(setter, &params.m_mids, step);
        let v = &params.m_volume;
        setter.begin_set_parameter(v);
        setter.set_parameter(v, (v.value() + 1.0).min(12.0));
        setter.end_set_parameter(v);
    }
    if fix_button(&mut f, "More sustain", a) {
        nudge(setter, &params.m_sustain, step);
    }
    if fix_button(&mut f, "More dirt", a) {
        nudge(setter, &params.m_gain, step);
    }
    if fix_button(&mut f, "Cleaner", a) {
        nudge(setter, &params.m_gain, -step);
    }
    if fix_button(&mut f, "Too washy", a) {
        nudge(setter, &params.m_space, -step);
    }
    if fix_button(&mut f, "Too dry", a) {
        nudge(setter, &params.m_space, step);
    }
}

/// How long the button listens (the level needs ~6 s of playing).
const SETUP_SECONDS: f32 = 8.0;

/// The guitar-level button. The level is learned automatically while you
/// play; the button asks for it to be learned again now. One fixed-size slot
/// shows, in turn: the button, a countdown bar while you play, the result.
fn setup_action(ed: &mut ToneEditor, ui: &mut Ui, action: Rect) {
    let p = ui.painter().clone();
    let radius = CornerRadius::same(10);
    if let Some((start, _)) = ed.setup {
        let left = SETUP_SECONDS - start.elapsed().as_secs_f32();
        if left > 0.0 {
            let done = 1.0 - left / SETUP_SECONDS;
            well(&p, action, 7);
            let fill = Rect::from_min_size(action.min, vec2(action.width() * done, action.height()));
            p.rect_filled(fill, radius, theme::ACCENT.gamma_multiply(0.45));
            p.text(
                action.center(),
                Align2::CENTER_CENTER,
                format!("Play for a few seconds…  {:.0}", left.ceil()),
                theme::semibold(12.5),
                theme::TEXT,
            );
            ui.ctx().request_repaint();
            return;
        }
        ed.setup = None;
        ed.setup_msg = Some(if ed.shared.level_learned.load(Ordering::Relaxed) {
            format!(
                "✓  Level set ({:+.0} dB)",
                ed.shared.auto_level_db.load(Ordering::Relaxed)
            )
        } else {
            "No guitar heard".to_owned()
        });
    }
    if let Some(msg) = ed.setup_msg.clone() {
        // Result chip: message left, "Try again" right, same size as the button.
        let ok = msg.starts_with('✓');
        let col = if ok { theme::GREEN } else { theme::RED };
        well(&p, action, 7);
        led(&p, action.left_center() + vec2(16.0, 0.0), true, col);
        p.text(
            action.left_center() + vec2(30.0, 0.0),
            Align2::LEFT_CENTER,
            msg.trim_start_matches('✓').trim(),
            theme::semibold(12.5),
            theme::TEXT,
        );
        let again = Rect::from_min_max(
            pos2(action.right() - 90.0, action.top()),
            action.max - vec2(14.0, 0.0),
        );
        let tip = if ok {
            "Measure again"
        } else {
            "Check the cable and the Input setting above, then try again"
        };
        if link(ui, again, "setup_again", "Try again", Align2::RIGHT_CENTER)
            .on_hover_text(tip)
            .clicked()
        {
            ed.setup_msg = None;
        }
        return;
    }
    let r = ui.interact(action, ui.id().with("setup_btn"), Sense::click());
    key(&p, action, 7, r.hovered(), r.is_pointer_button_down_on());
    p.text(
        action.center(),
        Align2::CENTER_CENTER,
        "Set up my guitar level",
        theme::semibold(12.5),
        theme::TEXT,
    );
    if r.on_hover_text(
        "Your level is learned automatically while you play. Press to learn it again (another guitar, or the interface gain changed).",
    )
    .clicked()
    {
        ed.shared.relearn_level.store(true, Ordering::Relaxed);
        ed.setup = Some((Instant::now(), 0.0));
        ed.setup_msg = None;
    }
}
