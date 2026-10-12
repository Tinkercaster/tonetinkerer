//! The audio callback must never allocate. `assert_no_alloc` aborts the test
//! if anything inside `Engine::process` (or a slot hand-over) touches the heap.

mod common;
use assert_no_alloc::*;
use common::*;
use tonetinkerer_dsp::assets::build_engine;
use tonetinkerer_dsp::presets::PRESETS;
use tonetinkerer_dsp::{ChainParams, amp, cab};

// `assert_no_alloc` only checks in builds with debug assertions; the test
// profile keeps them on (and is optimized), so run with plain `cargo test`.
#[cfg(debug_assertions)]
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

/// Push without ever reallocating (capacity is reserved up front).
fn keep<T>(v: &mut Vec<T>, x: T) {
    if v.len() < v.capacity() {
        v.push(x);
    } else {
        std::mem::forget(x);
    }
}

#[test]
fn process_never_allocates() {
    assert!(
        cfg!(debug_assertions),
        "allocation detector is inactive without debug assertions; run plain `cargo test`"
    );
    for &sr in &[44_100.0f32, 48_000.0, 96_000.0] {
        let input = pluck_phrase(sr, 2.0);
        for preset in PRESETS {
            let mut p = (preset.params)();
            let mut e = build_engine(sr);
            e.reset(&p);
            let mut l = input.clone();
            let mut r = input.clone();
            let custom = tonetinkerer_dsp::assets::custom_amp_slot(
                &{
                    let path =
                        std::env::temp_dir().join(format!("sky-rt-{}.nam", std::process::id()));
                    std::fs::write(&path, tonetinkerer_dsp::assets::AMP_MODELS[0].1).unwrap();
                    path
                },
                sr,
                e.amp_latency(),
            )
            .unwrap();
            let mut custom = Some(Box::new(custom));
            let mut retired = Vec::with_capacity(4);
            let mut k = 0usize;
            for (cl, cr) in l.chunks_mut(97).zip(r.chunks_mut(97)) {
                k += 1;
                assert_no_alloc(|| {
                    if k == 3 {
                        // Macros and chorus on the audio path too.
                        p = tonetinkerer_dsp::macros::apply(
                            &p,
                            &tonetinkerer_dsp::Macros {
                                gain: 0.5,
                                body: 0.5,
                                bright: -0.5,
                                mids: 0.3,
                                sustain: 0.5,
                                space: 0.7,
                                volume_db: -3.0,
                            },
                        );
                        p.chorus_enabled = !p.chorus_enabled;
                    }
                    if k == 5 {
                        p.amp_model = amp::SLOT_CUSTOM;
                        p.cab_ir = cab::SLOT_2X12;
                        p.delay_enabled = !p.delay_enabled;
                    }
                    if k == 9 {
                        if let Some(s) = custom.take() {
                            // Hand-over itself must be allocation-free; returned box is kept, not dropped.
                            if let Some(old) = e.swap_custom_amp(s) {
                                keep(&mut retired, old);
                            }
                        }
                    }
                    if k == 40 {
                        e.reset(&p);
                    }
                    e.process(cl, Some(cr), &p);
                    if let Some(old) = e.amp.take_retired() {
                        keep(&mut retired, old);
                    }
                });
            }
            drop(retired);
        }
    }
    let _ = ChainParams::default();
}
