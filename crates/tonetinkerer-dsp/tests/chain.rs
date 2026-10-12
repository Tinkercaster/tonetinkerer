//! Offline-rendering integration tests for the complete signal chain.

mod common;
use common::*;
use tonetinkerer_dsp::assets::build_engine;
use tonetinkerer_dsp::chain::InputMode;
use tonetinkerer_dsp::presets::PRESETS;
use tonetinkerer_dsp::{ChainParams, amp, cab};

const RATES: [f32; 4] = [44_100.0, 48_000.0, 88_200.0, 96_000.0];

#[test]
fn silence_in_silence_out() {
    for &sr in &RATES {
        for preset in PRESETS {
            let p = (preset.params)();
            let mut e = build_engine(sr);
            e.reset(&p);
            let input = vec![0.0; (sr * 1.5) as usize];
            let (l, r) = render(&mut e, &p, &input, 256);
            let settled = (sr * 0.5) as usize;
            assert!(
                peak(&l[settled..]) < 1e-4 && peak(&r[settled..]) < 1e-4,
                "{} @ {sr}: silence produced {}",
                preset.name,
                peak(&l[settled..])
            );
        }
    }
}

/// Hosts often reset a running plugin (instead of re-creating it) before an
/// offline render; the render must then be identical to a fresh engine's,
/// including the phase of every modulation LFO.
#[test]
fn reset_sounds_like_a_fresh_engine() {
    let sr = 48_000.0;
    let mut input = pluck_phrase(sr, 1.5);
    let pk = peak(&input);
    input.iter_mut().for_each(|x| *x *= 0.5 / pk);
    let mut all_fx = (PRESETS[0].params)();
    all_fx.chorus_enabled = true;
    all_fx.delay_enabled = true;
    all_fx.reverb_enabled = true;
    let cases = PRESETS
        .iter()
        .map(|p| (p.name, (p.params)()))
        .chain([("all time-based effects", all_fx)]);
    for (name, p) in cases {
        let mut fresh = build_engine(sr);
        fresh.reset(&p);
        let want = render(&mut fresh, &p, &input, 128);

        let mut used = build_engine(sr);
        used.reset(&p);
        // Run for an odd length so every LFO ends at an arbitrary phase.
        let _ = render(&mut used, &p, &input[..input.len() * 2 / 3 + 37], 128);
        used.reset(&p);
        let got = render(&mut used, &p, &input, 128);
        assert!(
            want.0 == got.0 && want.1 == got.1,
            "{name}: output after reset differs from a fresh engine"
        );
    }
}

/// A guitar on only one channel (an interface's input 1, or a host that
/// feeds a mono source into a stereo plugin) must sound exactly like the
/// same guitar duplicated on both channels.
#[test]
fn one_sided_input_sounds_like_duplicated_input() {
    let sr = 48_000.0;
    // Leading digital silence, as in a DAW render before the first note.
    let mut input = vec![0.0f32; 9_000];
    input.extend(pluck_phrase(sr, 1.5));
    let pk = peak(&input);
    input.iter_mut().for_each(|x| *x *= 0.5 / pk);
    let p = (PRESETS[0].params)();
    let mut a = build_engine(sr);
    a.reset(&p);
    let want = render(&mut a, &p, &input, 128);
    for side in [0, 1] {
        let mut e = build_engine(sr);
        e.reset(&p);
        let mut l = input.clone();
        let mut r = input.clone();
        if side == 0 { r.fill(0.0) } else { l.fill(0.0) }
        for (cl, cr) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            e.process(cl, Some(cr), &p);
        }
        let d = l.iter().zip(&want.0).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
        assert!(d < 1e-5, "guitar on {} channel only: max difference {d}", ["left", "right"][side]);
    }
}

/// A duplicated mono guitar where one copy stops mid-phrase (a host changing
/// its channel routing) must not dip or click: both channels carry the same
/// guitar, so the sound must not change at all.
#[test]
fn losing_a_duplicated_channel_is_seamless() {
    let sr = 48_000.0;
    let mut input = vec![0.0f32; 9_000];
    input.extend(pluck_phrase(sr, 1.5));
    let pk = peak(&input);
    input.iter_mut().for_each(|x| *x *= 0.5 / pk);
    let p = (PRESETS[0].params)();
    let mut a = build_engine(sr);
    a.reset(&p);
    let want = render(&mut a, &p, &input, 128);
    for side in [0, 1] {
        let mut e = build_engine(sr);
        e.reset(&p);
        let mut l = input.clone();
        let mut r = input.clone();
        let cut = input.len() / 2 / 128 * 128;
        if side == 0 { r[cut..].fill(0.0) } else { l[cut..].fill(0.0) }
        for (cl, cr) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            e.process(cl, Some(cr), &p);
        }
        let d = l.iter().zip(&want.0).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
        assert!(d < 1e-5, "{} copy stops mid-phrase: max difference {d}", ["right", "left"][side]);
    }
}

#[test]
fn every_preset_produces_sane_audio() {
    let sr = 48_000.0;
    // Realistic playing level: peaks at -6 dBFS.
    let mut input = pluck_phrase(sr, 4.0);
    let pk = peak(&input);
    input.iter_mut().for_each(|x| *x *= 0.5 / pk);
    for preset in PRESETS {
        let p = (preset.params)();
        let mut e = build_engine(sr);
        e.reset(&p);
        let (l, r) = render(&mut e, &p, &input, 128);
        assert!(
            all_finite(&l) && all_finite(&r),
            "{}: non-finite",
            preset.name
        );
        assert!(
            peak(&l) <= 1.0 && peak(&r) <= 1.0,
            "{}: exceeds 0 dBFS",
            preset.name
        );
        let level = 20.0 * rms(&l).log10();
        assert!(
            level > -36.0 && level < -3.0,
            "{}: implausible level {level} dBFS",
            preset.name
        );
    }
    // Loudness matching is judged on realistic plucked-string playing (the
    // same demo phrase `tonetinkerer-render levels` calibrates against).
    let line = tonetinkerer_dsp::testsig::demo_phrase(sr);
    let mut levels = Vec::new();
    for preset in PRESETS {
        let p = (preset.params)();
        let mut e = build_engine(sr);
        e.reset(&p);
        let (l, r) = render(&mut e, &p, &line, 128);
        levels.push((
            preset.name,
            10.0 * ((rms(&l).powi(2) + rms(&r).powi(2)) / 2.0).log10(),
        ));
    }
    let lo = levels.iter().map(|x| x.1).fold(f32::MAX, f32::min);
    let hi = levels.iter().map(|x| x.1).fold(f32::MIN, f32::max);
    for (n, l) in &levels {
        println!("LEVEL {n}: {l:.1} dBFS");
    }
    assert!(
        hi - lo < 2.0,
        "sounds are not loudness-matched (spread {:.1} dB): {levels:?}",
        hi - lo
    );
}

#[test]
fn extreme_input_is_bounded_by_the_limiter() {
    let sr = 48_000.0;
    // +20 dB over full scale square wave: a worst case far beyond any interface.
    let input: Vec<f32> = (0..(sr as usize * 2))
        .map(|i| if (i / 50) % 2 == 0 { 10.0 } else { -10.0 })
        .collect();
    for preset in PRESETS {
        let mut p = (preset.params)();
        p.input_gain_db = 24.0;
        p.amp_gain_db = 18.0;
        p.output_db = 12.0;
        let mut e = build_engine(sr);
        e.reset(&p);
        let (l, r) = render(&mut e, &p, &input, 512);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(
            peak(&l) <= 1.0 && peak(&r) <= 1.0,
            "{}: {}",
            preset.name,
            peak(&l)
        );
    }
}

#[test]
fn nan_and_inf_input_do_not_propagate() {
    let sr = 48_000.0;
    let p = (tonetinkerer_dsp::presets::find("Ambient Lead")
        .unwrap()
        .params)(); // long delay + reverb: worst case for poisoned feedback
    let mut e = build_engine(sr);
    e.reset(&p);
    let mut input = pluck_phrase(sr, 3.0);
    for i in (1000..input.len()).step_by(997) {
        input[i] = match i % 3 {
            0 => f32::NAN,
            1 => f32::INFINITY,
            _ => f32::NEG_INFINITY,
        };
    }
    let (l, r) = render(&mut e, &p, &input, 64);
    assert!(all_finite(&l) && all_finite(&r));
    // After the poison, clean input still produces normal output (state was not wiped to silence permanently).
    let clean = pluck_phrase(sr, 3.0);
    let (l2, _) = render(&mut e, &p, &clean, 64);
    assert!(rms(&l2) > 1e-3, "chain went silent after NaN input");
}

#[test]
fn stereo_effects_are_wide_and_mono_layout_works() {
    let sr = 48_000.0;
    let p = (tonetinkerer_dsp::presets::find("Ambient Lead")
        .unwrap()
        .params)(); // ping-pong delay + reverb
    let mut e = build_engine(sr);
    e.reset(&p);
    let input = pluck_phrase(sr, 4.0);
    let (l, r) = render(&mut e, &p, &input, 256);
    let diff: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
    assert!(rms(&diff) > 0.1 * rms(&l), "stereo image collapsed");

    // Mono-out layout: same engine, `None` for the right channel.
    let mut e = build_engine(sr);
    e.reset(&p);
    let mut m = input.clone();
    for c in m.chunks_mut(256) {
        e.process(c, None, &p);
    }
    assert!(all_finite(&m) && peak(&m) <= 1.0 && rms(&m) > 1e-3);
}

#[test]
fn auto_input_follows_the_guitar_channel() {
    let sr = 48_000.0;
    let p = ChainParams {
        input_mode: InputMode::Auto,
        delay_enabled: false,
        reverb_enabled: false,
        ..(PRESETS[0].params)()
    };
    let input = pluck_phrase(sr, 2.0);
    let run = |left_has_signal: bool| {
        let mut e = build_engine(sr);
        e.reset(&p);
        let silent = vec![0.0; input.len()];
        let (mut l, mut r) = if left_has_signal {
            (input.clone(), silent)
        } else {
            (silent, input.clone())
        };
        for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            e.process(cl, Some(cr), &p);
        }
        rms(&l)
    };
    let a = run(true);
    let b = run(false);
    assert!(
        a > 1e-3 && (20.0 * (a / b).log10()).abs() < 1.0,
        "left {a} vs right {b}"
    );
}

#[test]
fn variable_buffer_sizes_match_fixed_blocks() {
    for &sr in &RATES {
        let p = ChainParams {
            input_mode: InputMode::Left,
            ..(PRESETS[0].params)()
        };
        let input = pluck_phrase(sr, 2.0);
        let mut e1 = build_engine(sr);
        e1.reset(&p);
        let (a, _) = render(&mut e1, &p, &input, 256);

        let mut e2 = build_engine(sr);
        e2.reset(&p);
        let mut l = input.clone();
        let mut r = input.clone();
        let mut rng = Lcg(42);
        let mut off = 0;
        while off < l.len() {
            let n = match rng.next() % 4 {
                0 => 1 + (rng.next() % 8) as usize,
                1 => 1 + (rng.next() % 300) as usize,
                2 => 4096,
                _ => 31,
            }
            .min(l.len() - off);
            e2.process(&mut l[off..off + n], Some(&mut r[off..off + n]), &p);
            off += n;
        }
        let err = a
            .iter()
            .zip(&l)
            .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
        assert!(err < 1e-4, "{sr} Hz: block size changed output by {err}");
    }
}

#[test]
fn latency_is_reported_correctly() {
    for &sr in &RATES {
        // Linear-ish chain: clean model at low gain, everything else neutral.
        let p = ChainParams {
            input_mode: InputMode::Left,
            gate_enabled: false,
            comp_enabled: false,
            drive_enabled: false,
            amp_model: amp::SLOT_CLEAN,
            amp_gain_db: -18.0,
            cab_enabled: false,
            delay_enabled: false,
            reverb_enabled: false,
            limiter: false,
            ..ChainParams::default()
        };
        let mut e = build_engine(sr);
        e.reset(&p);
        let base = 0.25 * sr;
        let lat_48k = 12.0; // a 48 kHz model at 48 kHz: only the drive oversampler
        let n = (sr * 0.6) as usize;
        // Band-limited burst so the amp's own phase response barely matters.
        let input: Vec<f32> = (0..n)
            .map(|i| {
                let d = i as f32 - base;
                let w = (-(d / (0.002 * sr)).powi(2)).exp();
                0.05 * w * (2.0 * std::f32::consts::PI * 400.0 * d / sr).cos()
            })
            .collect();
        let (out, _) = render(&mut e, &p, &input, 128);
        let xc = |lag: usize| -> f32 { (0..n - lag).map(|i| input[i] * out[i + lag]).sum() };
        let best = (0..400)
            .max_by(|&a, &b| xc(a).abs().partial_cmp(&xc(b).abs()).unwrap())
            .unwrap();
        let reported = e.latency() as i64;
        assert!(
            (best as i64 - reported).abs() <= 3,
            "{sr} Hz: measured {best} reported {reported}"
        );
        if sr == 48_000.0 {
            assert_eq!(reported, lat_48k as i64);
        }
    }
}

#[test]
fn automation_stress_stays_finite_and_bounded() {
    let sr = 48_000.0;
    let mut e = build_engine(sr);
    let mut p = (PRESETS[0].params)();
    e.reset(&p);
    let input = pluck_phrase(sr, 6.0);
    let mut l = input.clone();
    let mut r = input.clone();
    let mut rng = Lcg(7);
    for (cl, cr) in l.chunks_mut(32).zip(r.chunks_mut(32)) {
        // Randomly jump every continuous parameter every 32 samples, and flip switches.
        p.input_gain_db = -12.0 + 24.0 * rng.unit();
        p.gate_threshold_db = -90.0 + 60.0 * rng.unit();
        p.wah_position = rng.unit();
        p.sustain = rng.unit();
        p.drive = rng.unit();
        p.drive_tone = rng.unit();
        p.amp_gain_db = -18.0 + 36.0 * rng.unit();
        p.amp_model = (rng.next() % 5) as usize;
        p.cab_ir = (rng.next() % 4) as usize;
        p.cab_low_cut = 20.0 + 380.0 * rng.unit();
        p.cab_high_cut = 2_000.0 + 18_000.0 * rng.unit();
        p.eq.bass_db = -12.0 + 24.0 * rng.unit();
        p.eq.mid_freq = 200.0 + 3_800.0 * rng.unit();
        p.delay.time_ms = 20.0 + 1_980.0 * rng.unit();
        p.delay.feedback = 0.95 * rng.unit();
        p.reverb.decay_s = 0.3 + 11.7 * rng.unit();
        p.reverb.predelay_ms = 200.0 * rng.unit();
        p.chorus.rate_hz = 0.1 + 4.9 * rng.unit();
        p.chorus.depth = rng.unit();
        p.eq.body_db = -12.0 + 24.0 * rng.unit();
        if rng.next() % 50 == 0 {
            p.chorus_enabled = !p.chorus_enabled;
            p.drive_enabled = !p.drive_enabled;
            p.delay_enabled = !p.delay_enabled;
            p.reverb_enabled = !p.reverb_enabled;
            p.wah_mode = [
                tonetinkerer_dsp::wah::WahMode::Off,
                tonetinkerer_dsp::wah::WahMode::Manual,
                tonetinkerer_dsp::wah::WahMode::Auto,
            ][(rng.next() % 3) as usize];
        }
        e.process(cl, Some(cr), &p);
    }
    assert!(all_finite(&l) && all_finite(&r));
    assert!(peak(&l) <= 1.0 && peak(&r) <= 1.0);
}

#[test]
fn smoothed_parameter_changes_do_not_click() {
    // A large output-level jump on a steady tone must ramp, not step:
    // compare against a twin engine that keeps the old level.
    let sr = 48_000.0;
    let p = ChainParams {
        input_mode: InputMode::Left,
        amp_model: amp::SLOT_CLEAN,
        delay_enabled: false,
        reverb_enabled: false,
        gate_enabled: false,
        ..ChainParams::default()
    };
    let tone: Vec<f32> = (0..(sr as usize))
        .map(|i| 0.05 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / sr).sin())
        .collect();
    let mut a = build_engine(sr);
    let mut b = build_engine(sr);
    a.reset(&p);
    b.reset(&p);
    let _ = render(&mut a, &p, &tone, 256);
    let _ = render(&mut b, &p, &tone, 256);
    let quiet = ChainParams {
        output_db: -24.0,
        ..p.clone()
    };
    let (changed, _) = render(&mut a, &quiet, &tone[..4096], 4096);
    let (same, _) = render(&mut b, &p, &tone[..4096], 4096);
    let sum = |v: &[f32]| v.iter().map(|x| x.abs()).sum::<f32>();
    let early = sum(&changed[..48]) / sum(&same[..48]);
    let late = sum(&changed[3000..]) / sum(&same[3000..]);
    assert!(early > 0.85, "level stepped instantly (ratio {early})");
    assert!(
        late < 0.1,
        "level never reached the new value (ratio {late})"
    );
}

#[test]
fn reset_and_model_switch_have_no_thump() {
    let sr = 48_000.0;
    let mut p = ChainParams {
        input_mode: InputMode::Left,
        delay_enabled: false,
        reverb_enabled: false,
        cab_enabled: false,
        ..(PRESETS[0].params)()
    };
    let mut e = build_engine(sr);
    e.reset(&p);
    let silence = vec![0.0; sr as usize];
    let (l, _) = render(&mut e, &p, &silence, 256);
    assert!(peak(&l) < 1e-4, "start-up transient {}", peak(&l));
    for m in [amp::SLOT_CLEAN, amp::SLOT_CRUNCH, amp::SLOT_LEAD] {
        p.amp_model = m;
        let (l, _) = render(&mut e, &p, &silence, 256);
        assert!(peak(&l) < 1e-4, "switch to {m}: transient {}", peak(&l));
    }
}

#[test]
fn custom_model_and_ir_hot_swap() {
    let sr = 48_000.0;
    let dir = std::env::temp_dir().join(format!("tonetinkerer-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("custom.nam");
    std::fs::write(&model_path, tonetinkerer_dsp::assets::AMP_MODELS[1].1).unwrap();
    let ir_path = dir.join("custom.wav");
    {
        // A 44.1 kHz, 24-bit, stereo IR: exercises resampling and channel selection.
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&ir_path, spec).unwrap();
        for i in 0..2_000 {
            let v = ((-(i as f32) / 120.0).exp() * (i as f32 * 0.3).cos() * 4_000_000.0) as i32;
            w.write_sample(v).unwrap();
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();
    }

    let mut e = build_engine(sr);
    let p = ChainParams {
        amp_model: amp::SLOT_CUSTOM,
        cab_ir: cab::SLOT_CUSTOM,
        ..(PRESETS[0].params)()
    };
    e.reset(&p);
    let input = pluck_phrase(sr, 3.0);
    let (before, _) = render(&mut e, &p, &input[..sr as usize], 256);
    assert!(
        rms(&before) > 1e-3,
        "missing custom slots must fall back to bundled ones"
    );

    let slot = tonetinkerer_dsp::assets::custom_amp_slot(&model_path, sr, e.amp_latency()).unwrap();
    assert!(e.swap_custom_amp(Box::new(slot)).is_none());
    let ir = tonetinkerer_dsp::cab::CabSlot::from_file(&ir_path, sr).unwrap();
    assert!(e.swap_custom_cab(Box::new(ir)).is_none());
    let (after, _) = render(&mut e, &p, &input, 256);
    assert!(all_finite(&after) && rms(&after) > 1e-3);

    // Swap again while the custom model is audible: the old one is retired after a crossfade.
    let slot2 =
        tonetinkerer_dsp::assets::custom_amp_slot(&model_path, sr, e.amp_latency()).unwrap();
    assert!(e.swap_custom_amp(Box::new(slot2)).is_none());
    let _ = render(&mut e, &p, &input[..(sr as usize / 2)], 256);
    assert!(
        e.amp.take_retired().is_some(),
        "retired model should be handed back after the fade"
    );

    // Bad files are clean errors, not panics.
    let bad = dir.join("bad.nam");
    std::fs::write(&bad, b"{ not json").unwrap();
    assert!(tonetinkerer_dsp::assets::custom_amp_slot(&bad, sr, 0).is_err());
    let bad_wav = dir.join("bad.wav");
    std::fs::write(&bad_wav, b"RIFFnope").unwrap();
    assert!(tonetinkerer_dsp::cab::CabSlot::from_file(&bad_wav, sr).is_err());
    assert!(tonetinkerer_dsp::assets::custom_amp_slot(&dir.join("missing.nam"), sr, 0).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

/// When the player mutes the strings, the gate must shut the interface's
/// background hiss off quickly. High-gain amps lift that hiss by 60 dB or
/// more, so a slowly fading gate turns it into a burst of full-level
/// distorted noise ("whoosh") after every stop.
#[test]
fn hiss_does_not_whoosh_after_the_gate_closes() {
    let sr = 48_000.0;
    let n = (sr * 2.5) as usize;
    let mut rng = Lcg(7);
    // -80 dBFS RMS white hiss: a quiet, ordinary interface noise floor.
    let g = 10f32.powf(-80.0 / 20.0) * 3f32.sqrt();
    let hiss: Vec<f32> = (0..n).map(|_| (rng.unit() - 0.5) * 2.0 * g).collect();
    let mut guitar = pluck_phrase(sr, 2.5);
    let pk = peak(&guitar);
    let mute = (sr * 1.2) as usize;
    for (i, x) in guitar.iter_mut().enumerate() {
        *x *= 0.35 / pk;
        if i > mute {
            // Palm on the strings: gone in a few tens of milliseconds.
            *x *= (-((i - mute) as f32) / (sr * 0.03)).exp();
        }
    }
    let noisy: Vec<f32> = guitar.iter().zip(&hiss).map(|(a, b)| a + b).collect();
    let mut bad = Vec::new();
    for preset in PRESETS {
        let p = (preset.params)();
        if !p.gate_enabled {
            continue;
        }
        let run = |input: &[f32]| {
            let mut e = build_engine(sr);
            e.reset(&p);
            render(&mut e, &p, input, 128).0
        };
        let with_hiss = run(&noisy);
        let clean = run(&guitar);
        // When the muted strings fall below the gate's closing level
        // (threshold minus its hysteresis) plus its 40 ms hold.
        let close = 10f32.powf((p.gate_threshold_db - 6.0) / 20.0);
        let w10 = (sr * 0.01) as usize;
        let below = (mute..n - w10).find(|&i| peak(&guitar[i..i + w10]) < close * 0.5).unwrap();
        let start = below + (sr * 0.15) as usize;
        let win = start..start + (sr * 0.2) as usize;
        let db = |v: &[f32]| 20.0 * rms(v).max(1e-6).log10();
        let (h, c) = (db(&with_hiss[win.clone()]), db(&clean[win]));
        if !(h < c + 6.0 || h < -66.0) {
            bad.push(format!("{}: {h:.0} dB (without hiss {c:.0} dB)", preset.name));
        }
    }
    assert!(bad.is_empty(), "hiss keeps the output up after the gate closed:\n{}", bad.join("\n"));
}

fn white(n: usize, db: f32, seed: u64) -> Vec<f32> {
    let mut rng = Lcg(seed);
    let g = 10f32.powf(db / 20.0) * 3f32.sqrt();
    (0..n).map(|_| (rng.unit() - 0.5) * 2.0 * g).collect()
}

/// A held note must ring through every sound until it has faded nearly to
/// the input's hiss; the gate must not chop it while it still sounds.
#[test]
fn held_notes_ring_until_they_reach_the_hiss() {
    let sr = 48_000.0;
    let n = (sr * 9.0) as usize;
    let start = (sr * 1.0) as usize;
    let mut input = white(n, -80.0, 11);
    // A 147 Hz note with harmonics, fading at 7 dB/s from -9 dBFS.
    for i in start..n {
        let t = (i - start) as f32 / sr;
        let a = 10f32.powf((-9.0 - 7.0 * t) / 20.0);
        for h in 1..6 {
            input[i] += a * (2.0 * std::f32::consts::PI * 147.0 * h as f32 * t).sin() / (h * h) as f32 * 0.7;
        }
    }
    // Through a high-gain amp a note far below -50 dBFS still rings at full
    // level, so it must keep sounding until it is only 16 dB above the hiss
    // (-64 dBFS, after ~7.9 s).
    let until = start + (sr * 7.85) as usize;
    let mut bad = Vec::new();
    for preset in PRESETS {
        let p = (preset.params)();
        if !p.gate_enabled {
            continue;
        }
        let run = |p: &ChainParams| {
            let mut e = build_engine(sr);
            e.reset(p);
            render(&mut e, p, &input, 128).0
        };
        let gated = run(&p);
        let open = run(&ChainParams { gate_enabled: false, ..p.clone() });
        // While the note is well above the hiss the gate must leave it alone.
        let w = (sr * 0.05) as usize;
        let db = |v: &[f32]| 20.0 * rms(v).max(1e-9).log10();
        let mut i = start + w;
        while i + w < until {
            let loss = db(&open[i..i + w]) - db(&gated[i..i + w]);
            if loss > 3.0 {
                bad.push(format!("{}: cut at {:.1} s ({loss:.0} dB)", preset.name, (i - start) as f32 / sr));
                break;
            }
            i += w;
        }
    }
    assert!(bad.is_empty(), "notes chopped by the gate:\n{}", bad.join("\n"));
}

/// On a noisy input (hiss above a sound's own gate threshold) the pauses
/// must still go quiet: the gate learns the hiss and closes on it, instead of
/// letting it wash through compressor and reverb forever.
#[test]
fn noisy_input_goes_quiet_in_pauses() {
    let sr = 48_000.0;
    let n = (sr * 10.0) as usize;
    let hiss = white(n, -64.0, 13);
    let mut guitar = vec![0.0f32; n];
    let phrase = pluck_phrase(sr, 2.5);
    let pk = peak(&phrase);
    let at = (sr * 2.0) as usize;
    for (i, v) in phrase.iter().enumerate() {
        guitar[at + i] = v * 0.35 / pk;
    }
    let mute = at + phrase.len();
    for (i, x) in guitar.iter_mut().enumerate().skip(mute - (sr * 0.4) as usize) {
        *x *= (-((i + (sr * 0.4) as usize - mute) as f32) / (sr * 0.03)).exp().min(1.0);
    }
    let noisy: Vec<f32> = guitar.iter().zip(&hiss).map(|(a, b)| a + b).collect();
    let mut bad = Vec::new();
    for preset in PRESETS {
        let p = (preset.params)();
        if !p.gate_enabled {
            continue;
        }
        let run = |x: &[f32]| {
            let mut e = build_engine(sr);
            e.reset(&p);
            render(&mut e, &p, x, 128).0
        };
        let a = run(&noisy);
        let b = run(&guitar);
        // What the hiss adds, 5 s into the pause (reverb tails included).
        let w = mute + (sr * 5.0) as usize..mute + (sr * 5.5) as usize;
        let diff: Vec<f32> = a[w.clone()].iter().zip(&b[w]).map(|(x, y)| x - y).collect();
        let db = 20.0 * rms(&diff).max(1e-9).log10();
        if db > -70.0 {
            bad.push(format!("{}: hiss at {db:.0} dBFS in the pause", preset.name));
        }
    }
    assert!(bad.is_empty(), "hiss keeps sounding in pauses:\n{}", bad.join("\n"));
}

fn ringing_chord(sr: f32, n: usize, start: usize) -> Vec<f32> {
    let mut chord = vec![0.0f32; n];
    for (k, f) in [82.4f32, 123.5, 164.8, 207.7, 246.9, 329.6].iter().enumerate() {
        let s0 = start + (sr * 0.012 * k as f32) as usize;
        for i in s0..n {
            let t = (i - s0) as f32 / sr;
            let env = (-t * 1.5).exp() * (t * 400.0).min(1.0);
            for h in 1..12 {
                chord[i] += 0.05 * env * (2.0 * std::f32::consts::PI * f * h as f32 * t).sin() / h as f32;
            }
        }
    }
    chord
}

/// Auto input must follow the input the guitar is on and never mix in an
/// unused input that only carries hiss (an empty instrument input with its
/// gain up is very common on two-input interfaces): it must sound exactly as
/// if that guitar input had been chosen by hand, from the first note on.
#[test]
fn auto_input_never_mixes_in_a_hissy_unused_input() {
    let sr = 48_000.0;
    let n = (sr * 5.0) as usize;
    let start = (sr * 2.0) as usize;
    let chord = ringing_chord(sr, n, start);
    let db = |v: &[f32]| 20.0 * rms(v).max(1e-9).log10();
    let mut bad = Vec::new();
    for in2 in [-70.0f32, -60.0, -50.0] {
        for name in ["Studio Clean", "Thrash Rhythm", "Ambient Lead"] {
            let base = (PRESETS.iter().find(|p| p.name == name).unwrap().params)();
            let hiss_db = |mode: InputMode| {
                let p = ChainParams { input_mode: mode, ..base.clone() };
                let run = |seed: u64| {
                    let mut l: Vec<f32> =
                        chord.iter().zip(&white(n, -75.0, seed)).map(|(a, b)| a + b).collect();
                    let mut r = white(n, in2, seed + 100);
                    let mut e = build_engine(sr);
                    e.reset(&p);
                    for (cl, cr) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
                        e.process(cl, Some(cr), &p);
                    }
                    l
                };
                let (a, b) = (run(1), run(2));
                // The hiss part (the chord cancels between the two renders).
                let w = start..n;
                let d: Vec<f32> = a[w.clone()].iter().zip(&b[w]).map(|(x, y)| x - y).collect();
                db(&d) - 3.0
            };
            let (left, auto) = (hiss_db(InputMode::Left), hiss_db(InputMode::Auto));
            if auto > left + 1.0 {
                bad.push(format!("{name}, input 2 hiss at {in2} dB: {left:.0} dB -> {auto:.0} dB in Auto"));
            }
        }
    }
    assert!(bad.is_empty(), "Auto mixes in the unused input's hiss:\n{}", bad.join("\n"));
}


/// Clean sounds follow the input level 1:1, distorted ones barely move. At
/// any interface gain every sound must still come out equally loud (the
/// automatic input level), not just at the level the sounds were matched at.
#[test]
fn every_sound_is_equally_loud_at_any_input_level() {
    let sr = 48_000.0;
    // Two passes of the demo phrase: the level is learned in the first.
    let mut phrase = tonetinkerer_dsp::testsig::demo_phrase(sr);
    phrase.extend(tonetinkerer_dsp::testsig::demo_phrase(sr));
    let half = phrase.len() / 2;
    let mut bad = Vec::new();
    for in_db in [-30.0f32, -24.0, 0.0] {
        let g = 10f32.powf(in_db / 20.0);
        let x: Vec<f32> = phrase.iter().map(|v| v * g).collect();
        let mut levels = Vec::new();
        for preset in PRESETS {
            let p = (preset.params)();
            let mut e = build_engine(sr);
            e.reset(&p);
            let (l, r) = render(&mut e, &p, &x, 256);
            let both: Vec<f32> = l[half..].iter().chain(&r[half..]).copied().collect();
            levels.push((preset.name, 20.0 * rms(&both).log10()));
        }
        for (name, db) in &levels {
            if (db + 16.0).abs() > 2.0 {
                bad.push(format!("input {in_db:+} dB: {name} at {db:.1} dBFS"));
            }
        }
    }
    assert!(bad.is_empty(), "sounds not level-matched (target -16 dBFS):\n{}", bad.join("\n"));
}

/// A long held note over a noisy input (hiss at -52 dBFS) must not be cut while it still rings clearly above the hiss.
#[test]
fn held_note_over_noisy_input_is_not_cut() {
    let sr = 48_000.0;
    let n = (sr * 12.0) as usize;
    let start = (sr * 1.0) as usize;
    let mut input = white(n, -52.0, 31);
    // A sustained 330 Hz note with bright harmonics, fading at 3 dB/s from -12 dBFS.
    for i in start..n {
        let t = (i - start) as f32 / sr;
        let a = 10f32.powf((-12.0 - 3.0 * t) / 20.0) * (t * 300.0).min(1.0);
        for h in 1..16 {
            input[i] += a * 0.6 / h as f32 * (2.0 * std::f32::consts::PI * 330.0 * h as f32 * t).sin();
        }
    }
    // Requirement: a note at least 6 dB above the hiss (RMS vs RMS) sounds
    // in full. This note's RMS is its amplitude -5.4 dB, so it is 6 dB above
    // the -52 dBFS hiss until its amplitude reaches -40.6 dB, after 9.5 s.
    let until = start + (sr * 9.5) as usize;
    gate_must_not_cut(&input, sr, start, until);
}

/// Legato: a picked note, then quieter hammer-on / pull-off notes with no
/// pick attack, over a noisy input (hiss at -52 dBFS). Every note must
/// sound in full.
#[test]
fn legato_notes_are_not_cut() {
    let sr = 48_000.0;
    let n = (sr * 6.0) as usize;
    let start = (sr * 1.0) as usize;
    let mut input = white(n, -52.0, 33);
    // (time, frequency, level dBFS): one picked note then five legato notes
    // 12-26 dB quieter, each ringing into the next. The quietest is still
    // 8 dB above the hiss (RMS), so every note must sound in full.
    let notes = [(0.0, 330.0, -12.0), (0.5, 370.0, -24.0), (1.0, 392.0, -28.0), (1.5, 370.0, -32.0), (2.0, 330.0, -36.0), (2.5, 294.0, -38.0)];
    for (k, &(t0, f, lv)) in notes.iter().enumerate() {
        let s0 = start + (sr * t0) as usize;
        let s1 = notes.get(k + 1).map_or(n, |nx| start + (sr * nx.0) as usize);
        for i in s0..s1 {
            let t = (i - s0) as f32 / sr;
            let a = 10f32.powf((lv - 4.0 * t) / 20.0) * (t * 600.0).min(1.0);
            for h in 1..10 {
                input[i] += a * 0.6 / h as f32 * (2.0 * std::f32::consts::PI * f * h as f32 * t).sin();
            }
        }
    }
    gate_must_not_cut(&input, sr, start, start + (sr * 3.0) as usize);
}

/// With the gate on, every sound must keep the playing in `start..until`
/// within 3 dB of the same sound with the gate off.
fn gate_must_not_cut(input: &[f32], sr: f32, start: usize, until: usize) {
    let db = |v: &[f32]| 20.0 * rms(v).max(1e-9).log10();
    let mut bad = Vec::new();
    for preset in PRESETS {
        let p = (preset.params)();
        if !p.gate_enabled {
            continue;
        }
        let run = |p: &ChainParams| {
            let mut e = build_engine(sr);
            e.reset(p);
            render(&mut e, p, input, 128).0
        };
        let gated = run(&p);
        let open = run(&ChainParams { gate_enabled: false, ..p.clone() });
        let w = (sr * 0.05) as usize;
        let mut i = start + w;
        while i + w < until {
            let loss = db(&open[i..i + w]) - db(&gated[i..i + w]);
            if loss > 3.0 {
                bad.push(format!("{}: cut at {:.2} s ({loss:.0} dB)", preset.name, (i - start) as f32 / sr));
                break;
            }
            i += w;
        }
    }
    assert!(bad.is_empty(), "notes cut by the gate:\n{}", bad.join("\n"));
}
