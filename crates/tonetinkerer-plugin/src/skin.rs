//! Skin: the window background. Drop artwork at `assets/skin/background.png`
//! (1120x700 or larger, same aspect) and rebuild; it is embedded and drawn
//! behind all panels with a darkening veil so text stays readable. Colours
//! live in `widgets::theme`.

use crate::widgets::theme;
use egui::{Color32, Mesh, Rect, TextureHandle, pos2};

#[cfg(skin_bg)]
const BACKGROUND: Option<&[u8]> = Some(include_bytes!("../../../assets/skin/background.png"));
#[cfg(not(skin_bg))]
const BACKGROUND: Option<&[u8]> = None;

pub fn load(ctx: &egui::Context) -> Option<TextureHandle> {
    let bytes = BACKGROUND?;
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("skin-background", color, egui::TextureOptions::LINEAR))
}

pub fn paint(ui: &egui::Ui, rect: Rect, tex: Option<&TextureHandle>) {
    let p = ui.painter();
    match tex {
        Some(t) => {
            p.image(
                t.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            // Light veil: the painting stays visible around the panels.
            p.rect_filled(rect, 0.0, Color32::from_rgba_premultiplied(4, 5, 14, 90));
            // Fade the art out behind the top bar and the footer line so the
            // wordmark and small text stay readable over the painting.
            let shade = Color32::from_rgba_premultiplied(5, 6, 16, 200);
            fade(p, Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 96.0)), shade, true);
            fade(p, Rect::from_min_max(pos2(rect.left(), rect.bottom() - 48.0), rect.max), shade, false);
            // Soft dark pool behind the wordmark (the painting's headstock sits there).
            let c = rect.left_top() + egui::vec2(110.0, 38.0);
            for i in 0..12 {
                let r = 200.0 - i as f32 * 14.0;
                p.add(egui::Shape::ellipse_filled(
                    c,
                    egui::vec2(r, r * 0.45),
                    Color32::from_rgba_premultiplied(2, 2, 8, 22),
                ));
            }
        }
        None => {
            // Deep vertical gradient with a warm glow from the top-left.
            let mut mesh = Mesh::default();
            let top = Color32::from_rgb(24, 22, 30);
            let bottom = theme::BG;
            mesh.colored_vertex(rect.left_top(), top);
            mesh.colored_vertex(rect.right_top(), Color32::from_rgb(16, 18, 26));
            mesh.colored_vertex(rect.right_bottom(), bottom);
            mesh.colored_vertex(rect.left_bottom(), bottom);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            p.add(mesh);
            for (r, a) in [(520.0, 10u8), (320.0, 14), (160.0, 16)] {
                p.circle_filled(
                    rect.left_top() + egui::vec2(140.0, 20.0),
                    r,
                    Color32::from_rgba_unmultiplied(242, 181, 68, a / 2),
                );
            }
        }
    }
}

/// Vertical fade from `col` at one edge of `r` to transparent at the other.
fn fade(p: &egui::Painter, r: Rect, col: Color32, from_top: bool) {
    let (a, b) = if from_top { (col, Color32::TRANSPARENT) } else { (Color32::TRANSPARENT, col) };
    let mut mesh = Mesh::default();
    mesh.colored_vertex(r.left_top(), a);
    mesh.colored_vertex(r.right_top(), a);
    mesh.colored_vertex(r.right_bottom(), b);
    mesh.colored_vertex(r.left_bottom(), b);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);
}
