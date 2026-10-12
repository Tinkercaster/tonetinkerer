//! Offscreen snapshots of the editor: runs the real egui UI without a host
//! window and rasterizes it on the CPU into PNGs under `target/snapshots/`.
//! The tests catch layout panics, and the images let anyone review the
//! screens without a DAW.

use crate::ToneTinkerer;
use crate::editor::{ToneEditor, View, WINDOW_SIZE};
use egui::epaint::{ClippedPrimitive, Primitive};
use egui::{Color32, ImageData, TextureId};
use nice_plug::context::PluginApi;
use nice_plug::context::gui::{GuiContext, GuiContextInner};
use nice_plug::params::internals::ParamPtr;
use nice_plug::plugin::PluginState;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tonetinkerer_dsp::presets::Genre;

const SCALE: f32 = 2.0;

/// A host stand-in: parameter changes go straight to the parameter.
struct NoHost;

impl GuiContextInner for NoHost {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }
    unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {}
    unsafe fn raw_set_parameter_normalized(&self, _: ParamPtr, _: f32) {}
    unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {}
    fn get_state(&self) -> PluginState {
        unimplemented!("not used by the editor")
    }
    fn set_state(&self, _: PluginState) {}
    fn request_restart(&self) {}
}

struct Texture {
    size: [usize; 2],
    pixels: Vec<Color32>,
}

/// Render a few frames of the editor and return the last one (RGBA, premultiplied → straight).
fn render(view: View, genre: Genre) -> image::RgbaImage {
    render_with(view, genre, |_| {})
}

fn render_with(view: View, genre: Genre, prep: impl Fn(&mut ToneEditor)) -> image::RgbaImage {
    let plugin = ToneTinkerer::default();
    let mut editor = ToneEditor::headless(plugin.params.clone(), plugin.shared.clone(), view, genre);
    prep(&mut editor);
    let gui = GuiContext::new(Arc::new(NoHost));
    let setter = gui.param_setter();
    let ctx = egui::Context::default();
    editor.init(&ctx);

    let (w, h) = (WINDOW_SIZE.width, WINDOW_SIZE.height);
    let mut textures: HashMap<TextureId, Texture> = HashMap::new();
    let mut last = None;
    for _ in 0..3 {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))),
            ..Default::default()
        };
        raw.viewports.entry(raw.viewport_id).or_default().native_pixels_per_point = Some(SCALE);
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| editor.draw(ui, &setter));
        });
        for (id, delta) in out.textures_delta.set.iter().flat_map(|(id, ds)| ds.iter().map(move |d| (id, d))) {
            let ImageData::Color(img) = &delta.image;
            let tex = textures.entry(*id).or_insert(Texture { size: img.size, pixels: vec![] });
            match delta.pos {
                None => {
                    tex.size = img.size;
                    tex.pixels = img.pixels.clone();
                }
                Some([x0, y0]) => {
                    for y in 0..img.size[1] {
                        for x in 0..img.size[0] {
                            tex.pixels[(y0 + y) * tex.size[0] + x0 + x] = img.pixels[y * img.size[0] + x];
                        }
                    }
                }
            }
        }
        for id in &out.textures_delta.free {
            textures.remove(id);
        }
        out.textures_delta.clear();
        last = Some(ctx.tessellate(out.shapes, out.pixels_per_point));
    }
    rasterize(&last.unwrap(), &textures, (w * SCALE) as usize, (h * SCALE) as usize)
}

fn rasterize(prims: &[ClippedPrimitive], textures: &HashMap<TextureId, Texture>, w: usize, h: usize) -> image::RgbaImage {
    // Premultiplied accumulation in gamma space (as egui's own software paths do).
    let mut buf = vec![[0f32; 4]; w * h];
    for p in prims {
        let Primitive::Mesh(mesh) = &p.primitive else { continue };
        let Some(tex) = textures.get(&mesh.texture_id) else { continue };
        let clip = p.clip_rect;
        let (cx0, cy0) = ((clip.min.x * SCALE).max(0.0) as usize, (clip.min.y * SCALE).max(0.0) as usize);
        let (cx1, cy1) = (((clip.max.x * SCALE).ceil() as usize).min(w), ((clip.max.y * SCALE).ceil() as usize).min(h));
        for tri in mesh.indices.chunks_exact(3) {
            let v = [&mesh.vertices[tri[0] as usize], &mesh.vertices[tri[1] as usize], &mesh.vertices[tri[2] as usize]];
            let pt = |i: usize| (v[i].pos.x * SCALE, v[i].pos.y * SCALE);
            let (a, b, c) = (pt(0), pt(1), pt(2));
            let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            if area.abs() < 1e-9 {
                continue;
            }
            let x0 = (a.0.min(b.0).min(c.0).floor().max(0.0) as usize).max(cx0);
            let x1 = (a.0.max(b.0).max(c.0).ceil() as usize).min(cx1);
            let y0 = (a.1.min(b.1).min(c.1).floor().max(0.0) as usize).max(cy0);
            let y1 = (a.1.max(b.1).max(c.1).ceil() as usize).min(cy1);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) / area;
                    let w1 = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
                    let t = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
                    let tx = ((u * tex.size[0] as f32) as usize).min(tex.size[0] - 1);
                    let ty = ((t * tex.size[1] as f32) as usize).min(tex.size[1] - 1);
                    let texel = tex.pixels[ty * tex.size[0] + tx].to_array();
                    let mut src = [0f32; 4];
                    for k in 0..4 {
                        let col = w0 * v[0].color[k] as f32 + w1 * v[1].color[k] as f32 + w2 * v[2].color[k] as f32;
                        src[k] = col / 255.0 * texel[k] as f32 / 255.0;
                    }
                    let d = &mut buf[y * w + x];
                    for k in 0..4 {
                        d[k] = src[k] + d[k] * (1.0 - src[3]);
                    }
                }
            }
        }
    }
    image::RgbaImage::from_fn(w as u32, h as u32, |x, y| {
        let p = buf[y as usize * w + x as usize];
        let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        image::Rgba([q(p[0]), q(p[1]), q(p[2]), 255])
    })
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A real screen, not a blank or single-colour frame.
fn assert_has_content(img: &image::RgbaImage, what: &str) {
    let mut colours = std::collections::HashSet::new();
    for p in img.pixels().step_by(7) {
        colours.insert(p.0);
    }
    assert!(colours.len() > 500, "{what}: only {} colours, the screen looks empty", colours.len());
}

#[test]
fn play_screen_renders_for_every_genre() {
    for genre in Genre::ALL {
        let img = render(View::Play, genre);
        assert_has_content(&img, genre.label());
        let name = genre.label().to_lowercase().replace([' ', '&'], "");
        img.save(out_dir().join(format!("play-{name}.png"))).unwrap();
    }
}

#[test]
fn advanced_screen_renders() {
    let img = render_with(View::Advanced, Genre::Lead, |e| {
        e.shared.noise_floor_db.store(-74.0, std::sync::atomic::Ordering::Relaxed)
    });
    assert_has_content(&img, "advanced");
    img.save(out_dir().join("advanced.png")).unwrap();
}

#[test]
fn guitar_setup_result_renders() {
    for (name, msg) in [("setup-none", "No guitar heard"), ("setup-ok", "✓  Level set (+3.5 dB)")] {
        let img = render_with(View::Play, Genre::Clean, |e| e.setup_msg = Some(msg.to_owned()));
        assert_has_content(&img, name);
        img.save(out_dir().join(format!("play-{name}.png"))).unwrap();
    }
}
