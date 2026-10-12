//! The macros must do what their labels say, measured on rendered audio.

mod common;
use common::*;
use tonetinkerer_dsp::assets::build_engine;
use tonetinkerer_dsp::presets::{PRESETS, find};
use tonetinkerer_dsp::{ChainParams, Macros, macros};

const SR: f32 = 48_000.0;

fn render_with(preset: &str, m: Macros) -> (Vec<f32>, Vec<f32>) {
    let p = macros::apply(&(find(preset).unwrap().params)(), &m);
    let mut e = build_engine(SR);
    e.reset(&p);
    let mut input = tonetinkerer_dsp::testsig::demo_phrase(SR);
    input.extend(std::iter::repeat_n(0.0, (SR * 2.0) as usize));
    render(&mut e, &p, &input, 256)
}

/// Energy share of a band (fraction of total), via a simple DFT on frames.
fn band_share(x: &[f32], lo: f32, hi: f32) -> f32 {
    let n = 4096;
    let (mut band, mut total) = (0.0f64, 0.0f64);
    for frame in x.chunks(n).filter(|f| f.len() == n) {
        for k in 1..n / 2 {
            let f = k as f32 * SR / n as f32;
            if f > 12_000.0 {
                break;
            }
            let (mut re, mut im) = (0.0f64, 0.0f64);
            // Sparse bins are enough for a share estimate: every 4th bin.
            if k % 4 != 0 {
                continue;
            }
            for (i, &v) in frame.iter().enumerate() {
                let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
                let a = 2.0 * std::f64::consts::PI * k as f64 * i as f64 / n as f64;
                re += v as f64 * w * a.cos();
                im -= v as f64 * w * a.sin();
            }
            let e = re * re + im * im;
            total += e;
            if f >= lo && f < hi {
                band += e;
            }
        }
    }
    (band / total.max(1e-30)) as f32
}

fn crest_db(x: &[f32]) -> f32 {
    20.0 * (peak(x) / rms(x).max(1e-9)).log10()
}

#[test]
fn body_thickens() {
    let thin = render_with(
        "Studio Clean",
        Macros {
            body: -1.0,
            ..Default::default()
        },
    )
    .0;
    let thick = render_with(
        "Studio Clean",
        Macros {
            body: 1.0,
            ..Default::default()
        },
    )
    .0;
    let (a, b) = (
        band_share(&thin[..SR as usize * 9], 80.0, 400.0),
        band_share(&thick[..SR as usize * 9], 80.0, 400.0),
    );
    assert!(b > a * 1.5, "Body+ should raise lows/low-mids: {a} -> {b}");
}

#[test]
fn bright_brightens() {
    let dark = render_with(
        "Classic Crunch",
        Macros {
            bright: -1.0,
            ..Default::default()
        },
    )
    .0;
    let bright = render_with(
        "Classic Crunch",
        Macros {
            bright: 1.0,
            ..Default::default()
        },
    )
    .0;
    let (a, b) = (
        band_share(&dark[..SR as usize * 9], 3_000.0, 10_000.0),
        band_share(&bright[..SR as usize * 9], 3_000.0, 10_000.0),
    );
    assert!(b > a * 1.5, "Bright+ should raise highs: {a} -> {b}");
}

#[test]
fn mids_push_mids() {
    let scooped = render_with(
        "Hard Rock",
        Macros {
            mids: -1.0,
            ..Default::default()
        },
    )
    .0;
    let pushed = render_with(
        "Hard Rock",
        Macros {
            mids: 1.0,
            ..Default::default()
        },
    )
    .0;
    let (a, b) = (
        band_share(&scooped[..SR as usize * 9], 500.0, 1_600.0),
        band_share(&pushed[..SR as usize * 9], 500.0, 1_600.0),
    );
    assert!(b > a * 1.3, "Mids+ should raise the mid band: {a} -> {b}");
}

#[test]
fn gain_saturates() {
    let clean = render_with(
        "Blues Breakup",
        Macros {
            gain: -1.0,
            ..Default::default()
        },
    )
    .0;
    let dirty = render_with(
        "Blues Breakup",
        Macros {
            gain: 1.0,
            ..Default::default()
        },
    )
    .0;
    let (a, b) = (
        crest_db(&clean[..SR as usize * 9]),
        crest_db(&dirty[..SR as usize * 9]),
    );
    assert!(
        b < a - 2.0,
        "Gain+ should compress peaks (crest factor): {a} dB -> {b} dB"
    );
}

#[test]
fn space_adds_tail() {
    let tail = |m: Macros| {
        let (l, _) = render_with("Classic Crunch", m);
        let start = (SR * 9.2) as usize; // after the phrase ends
        rms(&l[start..start + SR as usize])
    };
    let dry = tail(Macros {
        space: -1.0,
        ..Default::default()
    });
    let huge = tail(Macros {
        space: 1.0,
        ..Default::default()
    });
    assert!(
        huge > dry * 4.0,
        "Space+ should leave a longer tail: {dry} -> {huge}"
    );
}

#[test]
fn sustain_raises_decay_level() {
    // Later in a decaying note, more sustain means relatively more level left.
    let ratio = |m: Macros| {
        let (l, _) = render_with("Studio Clean", Macros { space: -1.0, ..m });
        let note = (SR * 2.9) as usize; // the long held note
        rms(&l[note + (SR * 1.6) as usize..note + (SR * 2.2) as usize])
            / rms(&l[note..note + (SR * 0.3) as usize])
    };
    let a = ratio(Macros {
        sustain: -1.0,
        ..Default::default()
    });
    let b = ratio(Macros {
        sustain: 1.0,
        ..Default::default()
    });
    assert!(
        b > a * 1.4,
        "Sustain+ should keep notes up longer: {a} -> {b}"
    );
}

#[test]
fn macro_automation_does_not_click() {
    // Jump every macro to an extreme mid-note; compare against a twin without the jump.
    let base = (find("Singing Lead").unwrap().params)();
    let input = tonetinkerer_dsp::testsig::demo_phrase(SR);
    let mut a = build_engine(SR);
    let mut b = build_engine(SR);
    a.reset(&base);
    b.reset(&base);
    let cut = (SR * 3.5) as usize;
    let _ = render(&mut a, &base, &input[..cut], 256);
    let _ = render(&mut b, &base, &input[..cut], 256);
    let jumped = macros::apply(
        &base,
        &Macros {
            gain: 1.0,
            body: -1.0,
            bright: 1.0,
            mids: -1.0,
            sustain: 1.0,
            space: 1.0,
            volume_db: -6.0,
        },
    );
    let (x, _) = render(&mut a, &jumped, &input[cut..cut + 4096], 4096);
    let (y, _) = render(&mut b, &base, &input[cut..cut + 4096], 4096);
    // The first 32 samples after the jump must still be close to the old sound.
    let d: f32 = x[..32]
        .iter()
        .zip(&y[..32])
        .map(|(p, q)| (p - q).abs())
        .fold(0.0, f32::max);
    assert!(d < 0.25 * peak(&y[..32]).max(0.01), "instant jump: {d}");
}

#[test]
fn every_sound_survives_macro_extremes() {
    let input = tonetinkerer_dsp::testsig::demo_phrase(SR);
    for preset in PRESETS {
        for v in [-1.0f32, 1.0] {
            let m = Macros {
                gain: v,
                body: v,
                bright: v,
                mids: v,
                sustain: v,
                space: v,
                volume_db: 12.0 * v,
            };
            let p: ChainParams = macros::apply(&(preset.params)(), &m);
            let mut e = build_engine(SR);
            e.reset(&p);
            let (l, r) = render(&mut e, &p, &input[..(SR * 3.0) as usize], 512);
            assert!(
                all_finite(&l) && all_finite(&r) && peak(&l) <= 1.0 && peak(&r) <= 1.0,
                "{} at {v}",
                preset.name
            );
        }
    }
}
