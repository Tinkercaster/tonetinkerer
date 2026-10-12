//! Custom egui widgets: rotary knobs, LED toggles, segmented selectors and
//! meters, all bound directly to nice-plug parameters via `ParamSetter` so
//! every gesture is a proper host automation gesture (begin / set / end).

use egui::{
    Align2, Color32, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};
use nice_plug::prelude::*;
use std::f32::consts::PI;

pub mod theme {
    use egui::Color32;
    /// Font families (registered in `editor::fonts`): Inter Medium / SemiBold.
    pub const MEDIUM: &str = "inter-medium";
    pub const SEMIBOLD: &str = "inter-semibold";
    pub fn regular(size: f32) -> egui::FontId {
        egui::FontId::proportional(size)
    }
    pub fn medium(size: f32) -> egui::FontId {
        egui::FontId::new(size, egui::FontFamily::Name(MEDIUM.into()))
    }
    pub fn semibold(size: f32) -> egui::FontId {
        egui::FontId::new(size, egui::FontFamily::Name(SEMIBOLD.into()))
    }

    pub const BG: Color32 = Color32::from_rgb(8, 9, 16);
    pub const PANEL: Color32 = Color32::from_rgb(20, 21, 34);
    pub const TEXT: Color32 = Color32::from_rgb(242, 243, 250);
    pub const MUTED: Color32 = Color32::from_rgb(160, 165, 190);
    pub const FAINT: Color32 = Color32::from_rgb(112, 117, 145);
    /// Main accent: electric violet.
    pub const ACCENT: Color32 = Color32::from_rgb(132, 102, 255);
    /// Second accent: cyan.
    pub const ACCENT2: Color32 = Color32::from_rgb(56, 200, 255);
    pub const GATE: Color32 = Color32::from_rgb(120, 170, 255);
    pub const WAH: Color32 = Color32::from_rgb(214, 102, 255);
    pub const SUSTAIN: Color32 = Color32::from_rgb(45, 212, 191);
    pub const DRIVE: Color32 = Color32::from_rgb(255, 92, 140);
    pub const CAB: Color32 = Color32::from_rgb(56, 200, 255);
    pub const CHORUS: Color32 = Color32::from_rgb(170, 120, 255);
    pub const DELAY: Color32 = Color32::from_rgb(80, 140, 255);
    pub const REVERB: Color32 = Color32::from_rgb(110, 120, 255);
    pub const OUTPUT: Color32 = Color32::from_rgb(200, 205, 230);
    pub const GREEN: Color32 = Color32::from_rgb(52, 211, 153);
    pub const RED: Color32 = Color32::from_rgb(255, 84, 112);
    /// Macro knob colours (Play screen), one per control.
    pub const MACRO: [Color32; 7] = [
        Color32::from_rgb(255, 92, 140),
        Color32::from_rgb(170, 110, 255),
        Color32::from_rgb(56, 200, 255),
        Color32::from_rgb(90, 130, 255),
        Color32::from_rgb(45, 212, 191),
        Color32::from_rgb(130, 150, 255),
        Color32::from_rgb(214, 102, 255),
    ];
}

pub const KNOB: f32 = 48.0;
/// Footprint of one `knob` (dial + two text lines).
pub const KNOB_SIZE: Vec2 = vec2(70.0, 86.0);
/// Height shared by segmented controls, pills, buttons and combo boxes.
pub const CONTROL_H: f32 = 28.0;
const DRAG_PIXELS: f32 = 220.0;
const START: f32 = PI * 0.75;
const SWEEP: f32 = PI * 1.5;

fn angle_point(c: Pos2, r: f32, a: f32) -> Pos2 {
    pos2(c.x + r * a.cos(), c.y + r * a.sin())
}

fn arc(c: Pos2, r: f32, a0: f32, a1: f32, stroke: Stroke) -> Shape {
    let n = (((a1 - a0).abs() / (PI / 48.0)).ceil() as usize).max(2);
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| angle_point(c, r, a0 + (a1 - a0) * i as f32 / n as f32))
        .collect();
    Shape::line(pts, stroke)
}

/// Value arc with a soft glow under it.
fn glow_arc(p: &egui::Painter, c: Pos2, r: f32, a0: f32, a1: f32, w: f32, col: Color32) {
    p.add(arc(c, r, a0, a1, Stroke::new(w * 2.6, col.gamma_multiply(0.16))));
    p.add(arc(c, r, a0, a1, Stroke::new(w, col)));
}

/// Dark knob cap with a top light, so it reads as a physical knob.
/// Filled circle with a vertical gradient (fan mesh, exact for a linear ramp).
fn gradient_circle(p: &egui::Painter, c: Pos2, r: f32, top: Color32, bottom: Color32) {
    let col = |y: f32| lerp_col(top, bottom, ((y - (c.y - r)) / (2.0 * r)).clamp(0.0, 1.0));
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(c, col(c.y));
    let n = 48u32;
    for i in 0..n {
        let a = i as f32 / n as f32 * PI * 2.0;
        let pt = angle_point(c, r, a);
        mesh.colored_vertex(pt, col(pt.y));
    }
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(mesh);
}

/// Knob cap finish.
#[derive(Clone, Copy)]
enum CapStyle {
    /// Dark anodised (logo).
    Dark,
    /// Coloured plastic cap in the section's colour (Advanced).
    Tinted(Color32),
    /// Big brushed-aluminium cap (the Play screen's main knobs).
    Silver,
}

fn scale(c: Color32, f: f32) -> Color32 {
    let m = |v: u8| (v as f32 * f).clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// Machined knob cap: lit-from-above skirt with grip ridges, a dished top
/// face and a specular edge along the top. Returns the pointer colour.
fn cap(p: &egui::Painter, c: Pos2, r: f32, hot: bool, style: CapStyle) -> Color32 {
    p.circle_filled(c + vec2(0.0, r * 0.12 + 2.0), r + 1.5, Color32::from_black_alpha(130));
    let lift = if hot { 1.12 } else { 1.0 };
    let (skirt_top, skirt_bot, face_top, face_bot, pointer) = match style {
        CapStyle::Dark => (
            Color32::from_rgb(78, 80, 104),
            Color32::from_rgb(20, 21, 32),
            Color32::from_rgb(30, 32, 46),
            Color32::from_rgb(52, 54, 74),
            theme::TEXT,
        ),
        CapStyle::Tinted(col) => (
            lerp_col(col, Color32::WHITE, 0.28),
            scale(col, 0.32),
            scale(col, 0.55),
            scale(col, 0.92),
            Color32::WHITE,
        ),
        CapStyle::Silver => (
            Color32::from_rgb(232, 233, 240),
            Color32::from_rgb(92, 94, 108),
            Color32::from_rgb(150, 152, 164),
            Color32::from_rgb(222, 224, 232),
            Color32::from_rgb(24, 25, 34),
        ),
    };
    gradient_circle(p, c, r, scale(skirt_top, lift), scale(skirt_bot, lift));
    // Grip ridges around the skirt.
    let ridges = ((r * 2.4) as usize).clamp(18, 56);
    for i in 0..ridges {
        let a = i as f32 / ridges as f32 * PI * 2.0;
        let alpha = if a.sin() < 0.0 { 60 } else { 100 };
        p.line_segment(
            [angle_point(c, r * 0.8, a), angle_point(c, r - 0.6, a)],
            Stroke::new(1.0, Color32::from_black_alpha(alpha)),
        );
    }
    p.circle_stroke(c, r, Stroke::new(1.0, Color32::from_black_alpha(170)));
    let face = r * 0.76;
    gradient_circle(p, c, face, scale(face_top, lift), scale(face_bot, lift));
    if matches!(style, CapStyle::Silver) {
        // Brushed finish: fine concentric rings.
        let mut rr = face - 1.5;
        let mut k = 0;
        while rr > 2.0 {
            let col = if k % 2 == 0 {
                Color32::from_white_alpha(22)
            } else {
                Color32::from_black_alpha(14)
            };
            p.circle_stroke(c, rr, Stroke::new(0.8, col));
            rr -= 1.8;
            k += 1;
        }
    }
    p.circle_stroke(c, face, Stroke::new(1.0, Color32::from_black_alpha(90)));
    p.add(arc(c, r - 0.5, PI * 1.15, PI * 1.85, Stroke::new(1.0, Color32::from_white_alpha(90))));
    pointer
}

/// Printed scale around a dial: `n` dots from start to end, the ends larger.
fn scale_ticks(p: &egui::Painter, c: Pos2, r0: f32, r1: f32, n: usize, col: Color32) {
    let r = (r0 + r1) / 2.0;
    for i in 0..n {
        let a = START + SWEEP * i as f32 / (n - 1) as f32;
        let end = i == 0 || i == n - 1;
        p.circle_filled(angle_point(c, r, a), if end { 1.6 } else { 1.0 }, col);
    }
}

/// Disabled look: desaturated and darker.
fn muted(c: Color32) -> Color32 {
    let l = (c.r() as f32 + c.g() as f32 + c.b() as f32) / 3.0;
    let mix = |v: u8| ((v as f32 * 0.35 + l * 0.65) * 0.5) as u8;
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}

/// Shared drag / scroll / double-click handling for both knob kinds.
fn knob_input<P: Param>(
    ui: &Ui,
    setter: &ParamSetter,
    param: &P,
    response: &Response,
    reset: f32,
    detent: bool,
) -> f32 {
    let id = response.id;
    let mut norm = param.unmodulated_normalized_value();
    if response.drag_started() {
        setter.begin_set_parameter(param);
        ui.memory_mut(|m| m.data.insert_temp(id, norm));
    }
    if response.dragged() {
        let scale = if ui.input(|i| i.modifiers.shift) {
            0.1
        } else {
            1.0
        };
        // Accumulate in memory so fine drags aren't lost to parameter stepping.
        let mut acc: f32 = ui.memory(|m| m.data.get_temp(id).unwrap_or(norm));
        acc = (acc - response.drag_delta().y / DRAG_PIXELS * scale).clamp(0.0, 1.0);
        ui.memory_mut(|m| m.data.insert_temp(id, acc));
        // Gentle detent at the centre ("as designed").
        norm = if detent && (acc - 0.5).abs() < 0.015 { 0.5 } else { acc };
        setter.set_parameter_normalized(param, norm);
    }
    if response.drag_stopped() {
        setter.end_set_parameter(param);
    }
    if response.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, reset);
        setter.end_set_parameter(param);
        norm = reset;
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let fine = ui.input(|i| i.modifiers.shift);
            let step = if fine { 0.002 } else { 0.01 } * scroll.signum();
            norm = (norm + step).clamp(0.0, 1.0);
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, norm);
            setter.end_set_parameter(param);
        }
    }
    norm
}

/// Rotary knob bound to any parameter. Drag vertically (Shift = fine),
/// scroll to nudge, double-click to reset to the default.
pub fn knob<P: Param>(
    ui: &mut Ui,
    setter: &ParamSetter,
    param: &P,
    label: &str,
    accent: Color32,
    enabled: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(KNOB_SIZE, Sense::click_and_drag());
    let norm = knob_input(ui, setter, param, &response, param.default_normalized_value(), false);

    let painter = ui.painter_at(rect.expand(8.0));
    let c = pos2(rect.center().x, rect.top() + KNOB / 2.0 + 2.0);
    let r = KNOB / 2.0;
    let col = if enabled { accent } else { muted(accent) };
    let active = response.hovered() || response.dragged();

    scale_ticks(&painter, c, r + 1.0, r + 3.5, 11, Color32::from_white_alpha(30));
    painter.add(arc(c, r - 3.0, START, START + SWEEP, Stroke::new(3.5, col.gamma_multiply(0.22))));
    glow_arc(&painter, c, r - 3.0, START, START + SWEEP * norm, 3.5, col);
    let cap_r = r - 9.0;
    let pointer = cap(&painter, c, cap_r, active, CapStyle::Tinted(col));
    let a = START + SWEEP * norm;
    painter.line_segment(
        [angle_point(c, cap_r * 0.3, a), angle_point(c, cap_r - 3.0, a)],
        Stroke::new(2.5, if enabled { pointer } else { theme::FAINT }),
    );

    let value = param.normalized_value_to_string(norm, true);
    let text_y = rect.top() + KNOB + 6.0;
    tracked_fit(
        ui,
        pos2(c.x, text_y + 1.0),
        Align2::CENTER_TOP,
        &label.to_uppercase(),
        9.5,
        if enabled { lerp_col(col, Color32::WHITE, 0.3) } else { theme::FAINT },
        1.0,
        KNOB_SIZE.x + 4.0,
    );
    // Value in a small display window.
    let win = Rect::from_center_size(pos2(c.x, text_y + 22.0), vec2(58.0, 15.0));
    well(&painter, win, 4);
    painter.text(
        win.center(),
        Align2::CENTER_CENTER,
        value,
        theme::medium(10.0),
        if active { theme::TEXT } else if enabled { col.gamma_multiply(0.9) } else { theme::FAINT },
    );
    response.on_hover_text_at_pointer(
        "Drag to turn · Shift = fine · Double-click = reset · Scroll = nudge",
    )
}

/// Round LED power switch for a BoolParam.
pub fn led_toggle(
    ui: &mut Ui,
    setter: &ParamSetter,
    param: &BoolParam,
    accent: Color32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    let on = param.value();
    if response.clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, !on);
        setter.end_set_parameter(param);
    }
    let p = ui.painter();
    let c = rect.center();
    if on {
        p.circle_filled(c, 10.0, accent.gamma_multiply(0.22));
        p.circle_filled(c, 6.0, accent);
        p.circle_filled(c - vec2(1.5, 1.5), 2.0, Color32::from_white_alpha(150));
    } else {
        p.circle_filled(c, 6.0, Color32::from_white_alpha(10));
        p.circle_stroke(c, 6.0, Stroke::new(1.5, Color32::from_white_alpha(40)));
    }
    if response.hovered() {
        p.circle_stroke(c, 10.0, Stroke::new(1.0, accent.gamma_multiply(0.6)));
    }
    response.on_hover_text(if on {
        "On - click to bypass"
    } else {
        "Bypassed - click to engage"
    })
}

/// Text with letter spacing, like printing on a hardware faceplate.
pub fn tracked(
    ui: &Ui,
    pos: Pos2,
    align: Align2,
    text: &str,
    font: egui::FontId,
    color: Color32,
    spacing: f32,
) -> Rect {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    let galley = ui.painter().layout_job(job);
    // The spacing after the last letter is not part of the visible word.
    let size = galley.size() - vec2(spacing, 0.0);
    let rect = align.anchor_size(pos, size);
    ui.painter().galley(rect.min, galley, color);
    rect
}

/// `tracked`, tightening the spacing (then the size) until it fits `max_w`.
#[allow(clippy::too_many_arguments)]
pub fn tracked_fit(
    ui: &Ui,
    pos: Pos2,
    align: Align2,
    text: &str,
    size: f32,
    color: Color32,
    spacing: f32,
    max_w: f32,
) -> Rect {
    let width = |sz: f32, sp: f32| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: theme::semibold(sz),
                extra_letter_spacing: sp,
                ..Default::default()
            },
        );
        ui.painter().layout_job(job).size().x - sp
    };
    let (mut sz, mut sp) = (size, spacing);
    while width(sz, sp) > max_w && sz > 8.0 {
        if sp > 0.25 {
            sp -= 0.25;
        } else {
            sz -= 0.5;
        }
    }
    tracked(ui, pos, align, text, theme::semibold(sz), color, sp)
}

fn lerp_col(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

/// Rounded rect with a vertical gradient (a fan mesh: colour is linear in y,
/// so the interpolation is exact). Callers stroke the edge on top.
pub fn gradient_rect(p: &egui::Painter, r: Rect, radius: f32, top: Color32, bottom: Color32) {
    let col = |y: f32| lerp_col(top, bottom, ((y - r.top()) / r.height()).clamp(0.0, 1.0));
    let rad = radius.min(r.height() / 2.0).min(r.width() / 2.0);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(r.center(), col(r.center().y));
    let n = 6;
    let corners = [
        (r.right() - rad, r.bottom() - rad, 0.0),
        (r.left() + rad, r.bottom() - rad, PI * 0.5),
        (r.left() + rad, r.top() + rad, PI),
        (r.right() - rad, r.top() + rad, PI * 1.5),
    ];
    for (cx, cy, a0) in corners {
        for i in 0..=n {
            let a = a0 + PI * 0.5 * i as f32 / n as f32;
            let pt = pos2(cx + rad * a.cos(), cy + rad * a.sin());
            mesh.colored_vertex(pt, col(pt.y));
        }
    }
    let count = 4 * (n + 1) as u32;
    for i in 0..count {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % count);
    }
    p.add(mesh);
}

/// Recessed slot that keys and selectors sit in.
pub fn well(p: &egui::Painter, r: Rect, radius: u8) {
    gradient_rect(
        p,
        r,
        radius as f32,
        Color32::from_rgba_premultiplied(2, 2, 6, 230),
        Color32::from_rgba_premultiplied(8, 9, 16, 210),
    );
    p.rect_stroke(
        r,
        CornerRadius::same(radius),
        Stroke::new(1.0, Color32::from_black_alpha(150)),
        StrokeKind::Inside,
    );
    // Light catching the lower lip of the slot.
    p.line_segment(
        [
            pos2(r.left() + radius as f32, r.bottom() + 0.5),
            pos2(r.right() - radius as f32, r.bottom() + 0.5),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(14)),
    );
}

/// Raised key: soft gradient, dark edge, a highlight along the top.
pub fn key(p: &egui::Painter, r: Rect, radius: u8, hot: bool, down: bool) {
    let (mut top, mut bottom) = if hot {
        (Color32::from_rgb(64, 66, 94), Color32::from_rgb(39, 41, 60))
    } else {
        (Color32::from_rgb(52, 54, 78), Color32::from_rgb(32, 34, 50))
    };
    if down {
        std::mem::swap(&mut top, &mut bottom);
    }
    p.rect_filled(
        r.translate(vec2(0.0, 1.5)),
        CornerRadius::same(radius),
        Color32::from_black_alpha(90),
    );
    gradient_rect(p, r, radius as f32, top, bottom);
    p.rect_stroke(
        r,
        CornerRadius::same(radius),
        Stroke::new(1.0, Color32::from_black_alpha(160)),
        StrokeKind::Outside,
    );
    if !down {
        p.line_segment(
            [
                pos2(r.left() + radius as f32, r.top() + 0.5),
                pos2(r.right() - radius as f32, r.top() + 0.5),
            ],
            Stroke::new(1.0, Color32::from_white_alpha(34)),
        );
    }
}

/// A small glowing light (on) or a dark lens (off).
pub fn led(p: &egui::Painter, c: Pos2, on: bool, col: Color32) {
    if on {
        p.circle_filled(c, 6.0, col.gamma_multiply(0.18));
        p.circle_filled(c, 3.2, col);
        p.circle_filled(c - vec2(0.8, 0.8), 1.2, Color32::from_white_alpha(170));
    } else {
        p.circle_filled(c, 3.2, Color32::from_rgb(14, 15, 24));
        p.circle_stroke(c, 3.2, Stroke::new(1.0, Color32::from_white_alpha(22)));
    }
}

/// The lit strip under the label of a selected key.
pub fn lit_bar(p: &egui::Painter, r: Rect, col: Color32) {
    let w = (r.width() * 0.34).clamp(12.0, 34.0);
    let y = r.bottom() - 2.5;
    let a = pos2(r.center().x - w / 2.0, y);
    let b = pos2(r.center().x + w / 2.0, y);
    p.line_segment([a, b], Stroke::new(4.0, col.gamma_multiply(0.22)));
    p.line_segment([a, b], Stroke::new(1.5, col));
}

/// Lay out `text` at the largest size from `size` down to 9.5 pt that fits `max_w`.
fn fitted(ui: &Ui, text: &str, size: f32, max_w: f32) -> egui::FontId {
    let mut s = size;
    while s > 9.5 {
        let w = ui
            .painter()
            .layout_no_wrap(text.to_owned(), theme::medium(s), Color32::WHITE)
            .size()
            .x;
        if w <= max_w {
            break;
        }
        s -= 0.5;
    }
    theme::medium(s)
}

/// Segmented selector for an EnumParam.
pub fn segmented<T: Enum + PartialEq + Copy + 'static>(
    ui: &mut Ui,
    setter: &ParamSetter,
    param: &EnumParam<T>,
    accent: Color32,
    width: f32,
) -> Option<T> {
    segmented_labeled(ui, setter, param, T::variants(), accent, width)
}

/// Segmented selector with display labels (e.g. short names for long host names).
/// A recessed slot with equal keys; the chosen one is raised and lit.
/// Labels shrink to fit their key instead of spilling out.
pub fn segmented_labeled<T: Enum + PartialEq + Copy + 'static>(
    ui: &mut Ui,
    setter: &ParamSetter,
    param: &EnumParam<T>,
    names: &[&str],
    accent: Color32,
    width: f32,
) -> Option<T> {
    let n = names.len();
    let h = CONTROL_H;
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    let seg_w = (width - 4.0) / n as f32;
    let cur = param.value().to_index();
    let mut picked = None;
    let painter = ui.painter().clone();
    well(&painter, rect, 6);
    for (i, name) in names.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(rect.left() + 2.0 + seg_w * i as f32, rect.top() + 2.0),
            vec2(seg_w, h - 4.0),
        )
        .shrink2(vec2(1.0, 0.0));
        let resp = ui.interact(r, ui.id().with(("seg", param.name(), i)), Sense::click());
        let selected = i == cur;
        if selected {
            key(&painter, r, 5, resp.hovered(), false);
            lit_bar(&painter, r, accent);
        }
        painter.text(
            r.center() - vec2(0.0, if selected { 2.0 } else { 0.0 }),
            Align2::CENTER_CENTER,
            *name,
            fitted(ui, name, 12.0, r.width() - 10.0),
            if selected || resp.hovered() {
                theme::TEXT
            } else {
                theme::MUTED
            },
        );
        if resp.clicked() && !selected {
            let v = T::from_index(i);
            setter.begin_set_parameter(param);
            setter.set_parameter(param, v);
            setter.end_set_parameter(param);
            picked = Some(v);
        }
    }
    picked
}

/// Latching key with an indicator light (e.g. "Ping-Pong").
pub fn pill(
    ui: &mut Ui,
    setter: &ParamSetter,
    param: &BoolParam,
    label: &str,
    accent: Color32,
) -> Response {
    let font = theme::medium(12.0);
    let w = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let (rect, response) = ui.allocate_exact_size(vec2(w + 40.0, CONTROL_H), Sense::click());
    let on = param.value();
    if response.clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, !on);
        setter.end_set_parameter(param);
    }
    let p = ui.painter();
    key(p, rect, 6, response.hovered(), response.is_pointer_button_down_on());
    led(p, pos2(rect.left() + 14.0, rect.center().y), on, accent);
    p.text(
        pos2(rect.left() + 25.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        if on { theme::TEXT } else { theme::MUTED },
    );
    response
}

/// Vertical LED-segment peak meter in dBFS (-60..+3). `zone` marks a
/// recommended range with small green ticks beside the column.
pub fn meter(
    ui: &mut Ui,
    label: &str,
    peaks: &[f32],
    height: f32,
    zone: Option<(f32, f32)>,
) -> Response {
    let seg_w = 7.0;
    let w = peaks.len() as f32 * (seg_w + 2.0) + 4.0;
    let (rect, response) = ui.allocate_exact_size(vec2(w.max(26.0) + 6.0, height + 16.0), Sense::hover());
    let p = ui.painter();
    let area = Rect::from_min_size(pos2(rect.center().x - w / 2.0, rect.top()), vec2(w, height));
    well(p, area, 3);
    let inner = area.shrink(2.0);
    let n = 12;
    let seg_h = (inner.height() + 1.0) / n as f32 - 1.0;
    let to_db = |i: usize| -60.0 + 63.0 * (i as f32 + 0.5) / n as f32;
    if let Some((lo, hi)) = zone {
        for i in 0..n {
            let db = to_db(i);
            if db >= lo && db <= hi {
                let y = inner.bottom() - (i as f32 + 0.5) * (seg_h + 1.0);
                p.line_segment(
                    [pos2(area.left() - 4.0, y), pos2(area.left() - 1.5, y)],
                    Stroke::new(1.5, theme::GREEN),
                );
            }
        }
    }
    for (ch, &peak) in peaks.iter().enumerate() {
        let x = inner.left() + ch as f32 * (seg_w + 2.0);
        let db = 20.0 * peak.max(1e-6).log10();
        for i in 0..n {
            let y1 = inner.bottom() - i as f32 * (seg_h + 1.0);
            let r = Rect::from_min_max(pos2(x, y1 - seg_h), pos2(x + seg_w, y1));
            let seg_db = to_db(i);
            let col = if seg_db > -1.5 {
                theme::RED
            } else if seg_db > -9.0 {
                theme::ACCENT
            } else {
                theme::ACCENT2
            };
            let on = peak > 1e-6 && db >= seg_db - 63.0 / n as f32 / 2.0;
            if on {
                p.rect_filled(r.expand(1.0), CornerRadius::same(1), col.gamma_multiply(0.25));
                p.rect_filled(r, CornerRadius::same(1), col);
            } else {
                p.rect_filled(r, CornerRadius::same(1), col.gamma_multiply(0.09));
            }
        }
    }
    tracked(
        ui,
        pos2(rect.center().x, area.bottom() + 4.0),
        Align2::CENTER_TOP,
        label,
        theme::semibold(8.5),
        theme::FAINT,
        1.2,
    );
    response
}

/// Panel used by both views: dark glass with a bevel highlight on top.
pub fn panel(ui: &Ui, rect: Rect, radius: u8) {
    let p = ui.painter();
    p.rect_filled(
        rect.translate(vec2(0.0, 8.0)).expand(2.0),
        CornerRadius::same(radius + 2),
        Color32::from_black_alpha(60),
    );
    gradient_rect(
        p,
        rect,
        radius as f32,
        Color32::from_rgba_premultiplied(16, 17, 30, 214),
        Color32::from_rgba_premultiplied(9, 10, 18, 222),
    );
    p.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(1.0, Color32::from_black_alpha(140)),
        StrokeKind::Outside,
    );
    p.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(1.0, Color32::from_white_alpha(9)),
        StrokeKind::Inside,
    );
    p.line_segment(
        [
            pos2(rect.left() + radius as f32, rect.top() + 0.5),
            pos2(rect.right() - radius as f32, rect.top() + 0.5),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(26)),
    );
}

/// A card at a fixed rect: a header row (LED or colour dot, title) and a body.
/// Returns (enabled, header rect, body rect); everything in a card is placed
/// relative to these so rows of cards line up.
pub fn card_at(
    ui: &mut Ui,
    rect: Rect,
    title: &str,
    accent: Color32,
    toggle: Option<(&ParamSetter, &BoolParam)>,
) -> (bool, Rect, Rect) {
    let enabled = toggle.map_or(true, |(_, p)| p.value());
    panel(ui, rect, 10);
    // Each section wears its own colour, strongest at the top.
    gradient_rect(
        ui.painter(),
        rect.shrink(1.0),
        9.0,
        accent.gamma_multiply(if enabled { 0.16 } else { 0.06 }),
        accent.gamma_multiply(0.015),
    );
    let inner = rect.shrink2(vec2(16.0, 14.0));
    let header = Rect::from_min_size(inner.min, vec2(inner.width(), 24.0));
    let mark = Rect::from_min_size(header.min, vec2(22.0, 24.0));
    match toggle {
        Some((setter, p)) => {
            let mut c = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(mark)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            led_toggle(&mut c, setter, p, accent);
        }
        None => led(ui.painter(), mark.center(), true, accent),
    }
    tracked(
        ui,
        pos2(mark.right() + 6.0, header.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::semibold(11.5),
        if enabled { theme::TEXT } else { theme::MUTED },
        1.4,
    );
    etched(
        ui.painter(),
        pos2(rect.left() + 1.0, header.bottom() + 4.0),
        pos2(rect.right() - 1.0, header.bottom() + 4.0),
    );
    let body = Rect::from_min_max(pos2(inner.left(), header.bottom() + 12.0), inner.max);
    (enabled, header, body)
}

/// Big centre-detented macro knob (-1..+1, 0 = as designed). The arc grows
/// from 12 o'clock; plain-language words sit under each end. `width` is the
/// column width; the dial shrinks to fit narrow columns.
#[allow(clippy::too_many_arguments)]
pub fn macro_knob(
    ui: &mut Ui,
    setter: &ParamSetter,
    param: &FloatParam,
    label: &str,
    low: &str,
    high: &str,
    accent: Color32,
    width: f32,
) -> Response {
    let d = (width - 34.0).clamp(56.0, 84.0);
    let size = vec2(width, d + 54.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let norm = knob_input(ui, setter, param, &response, 0.5, true);

    let p = ui.painter_at(rect.expand(8.0));
    let c = pos2(rect.center().x, rect.top() + d / 2.0 + 4.0);
    let r = d / 2.0;
    let active = response.hovered() || response.dragged();
    let changed = (norm - 0.5).abs() > 0.001;
    // Coloured track, bright arc growing from 12 o'clock ("as designed").
    let ring = r - 5.0;
    scale_ticks(&p, c, ring + 4.5, ring + 8.0, 11, Color32::from_white_alpha(36));
    p.add(arc(c, ring, START, START + SWEEP, Stroke::new(5.0, accent.gamma_multiply(0.2))));
    let mid = START + SWEEP * 0.5;
    let a = START + SWEEP * norm;
    if changed {
        glow_arc(&p, c, ring, mid.min(a), mid.max(a), 5.0, accent);
    }
    p.circle_filled(angle_point(c, ring, a), 4.5, accent);
    p.circle_filled(angle_point(c, ring, a), 2.0, Color32::WHITE);
    let cap_r = r - 10.0;
    let pointer = cap(&p, c, cap_r, active, CapStyle::Silver);
    p.line_segment(
        [angle_point(c, cap_r * 0.35, a), angle_point(c, cap_r - 4.0, a)],
        Stroke::new(3.0, pointer),
    );
    let y = c.y + r + 8.0;
    p.text(pos2(c.x, y), Align2::CENTER_TOP, label, theme::semibold(14.0), theme::TEXT);
    // Under the name: what the knob does, or its value once moved.
    let (line, col) = if changed || active {
        (param.normalized_value_to_string(norm, true), accent)
    } else {
        (format!("{low} – {high}"), theme::MUTED)
    };
    p.text(pos2(c.x, y + 20.0), Align2::CENTER_TOP, line, theme::regular(11.5), col);
    response.on_hover_text_at_pointer("Drag to turn · Double-click = back to the sound as designed")
}

/// Quick-fix key. Returns true when clicked.
pub fn fix_button(ui: &mut Ui, text: &str, accent: Color32) -> bool {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::medium(12.5), theme::TEXT);
    let (rect, resp) =
        ui.allocate_exact_size(vec2(galley.size().x + 30.0, 32.0), Sense::click());
    let p = ui.painter();
    let down = resp.is_pointer_button_down_on();
    key(p, rect, 6, resp.hovered(), down);
    if down {
        lit_bar(p, rect, accent);
    }
    p.text(
        rect.center() + vec2(0.0, if down { 0.5 } else { -0.5 }),
        Align2::CENTER_CENTER,
        text,
        theme::medium(12.5),
        if resp.hovered() { theme::TEXT } else { theme::MUTED },
    );
    resp.clicked()
}

/// Product mark: a knob ring (violet into cyan) with its pointer.
pub fn logo_mark(p: &egui::Painter, c: Pos2, r: f32) {
    p.circle_filled(c, r + 3.0, Color32::from_black_alpha(110));
    p.add(arc(c, r, START, START + SWEEP, Stroke::new(2.5, Color32::from_white_alpha(18))));
    let end = 0.72;
    let n = 24;
    for i in 0..n {
        let t0 = i as f32 / n as f32 * end;
        let t1 = (i + 1) as f32 / n as f32 * end;
        let col = lerp_col(theme::ACCENT, theme::ACCENT2, t1 / end);
        p.add(arc(c, r, START + SWEEP * t0, START + SWEEP * t1 + 0.01, Stroke::new(2.5, col)));
    }
    cap(p, c, r - 5.0, false, CapStyle::Dark);
    let a = START + SWEEP * end;
    p.line_segment(
        [angle_point(c, (r - 5.0) * 0.3, a), angle_point(c, r - 7.5, a)],
        Stroke::new(2.0, theme::TEXT),
    );
}

/// Etched rule: a dark groove with a light lip under it.
pub fn etched(p: &egui::Painter, a: Pos2, b: Pos2) {
    p.line_segment([a, b], Stroke::new(1.0, Color32::from_black_alpha(140)));
    p.line_segment([a + vec2(0.0, 1.0), b + vec2(0.0, 1.0)], Stroke::new(1.0, Color32::from_white_alpha(12)));
}

/// Recessed display window with a faint glass sheen on its upper half.
pub fn display(p: &egui::Painter, r: Rect, radius: u8) {
    well(p, r, radius);
    let sheen = Rect::from_min_max(r.min + vec2(1.0, 1.0), pos2(r.right() - 1.0, r.center().y));
    gradient_rect(
        p,
        sheen,
        radius as f32,
        Color32::from_white_alpha(7),
        Color32::TRANSPARENT,
    );
}
