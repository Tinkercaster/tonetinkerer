//! `tonetinkerer-render`: offline tooling.
//!
//! Subcommands:
//!   gen-train <out.wav> [seconds]              synthetic guitar DI for NAM training
//!   refamp <clean|crunch|lead> <in> <out>      render the reference circuit (training target)
//!   calibrate <model.nam> <voice> <di.wav> <out.nam> <name>  verify vs reference + write metadata
//!   render <preset|default> <in.wav|demo> <out.wav> [rate] [block]  full plugin chain, offline
//!   demos <dir>                                 render every preset on the demo phrase
//!   latency <rate>                              plugin latency at a sample rate
//!   compare <a.wav> <b.wav> <shift> <secs>      max diff after shifting b by `shift` samples
//!   check <file.wav>                            finite / peak / rms / stereo-width report
//!   resynth <f0.csv> <out.wav>                  pitch-matched DI from a reference pitch track
//!   gen-irs <dir>                              synthesize the bundled cabinet IRs

use std::process::ExitCode;

mod irgen;
mod refamp;
use tonetinkerer_dsp::testsig as trainsig;

/// Interleaved samples as f32 (float or integer WAVs), channel count, rate.
fn read_wav(path: &str) -> (Vec<f32>, usize, u32) {
    let mut r = hound::WavReader::open(path).unwrap_or_else(|e| panic!("open {path}: {e}"));
    let spec = r.spec();
    let data: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.unwrap() as f32 * scale)
                .collect()
        }
    };
    (data, spec.channels as usize, spec.sample_rate)
}

fn read_all(path: &str) -> (Vec<f32>, usize) {
    let (data, ch, _) = read_wav(path);
    (data, ch)
}

fn read_mono(path: &str) -> (Vec<f32>, u32) {
    let (data, ch, sr) = read_wav(path);
    (data.chunks(ch).map(|c| c[0]).collect(), sr)
}

fn write_mono(path: &str, data: &[f32], sr: u32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sr,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w =
        hound::WavWriter::create(path, spec).unwrap_or_else(|e| panic!("create {path}: {e}"));
    for &s in data {
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen-train") if args.len() >= 3 => {
            let secs: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(240.0);
            let sig = trainsig::generate(48_000.0, secs, 0x5ce7);
            write_mono(&args[2], &sig, 48_000);
            eprintln!("wrote {} ({secs} s)", args[2]);
        }
        Some("refamp") if args.len() >= 5 => {
            let voice = refamp::Voice::parse(&args[2]).expect("voice: clean|crunch|lead");
            let (input, sr) = read_mono(&args[3]);
            let out = refamp::render_aligned(voice, sr as f32, &input);
            let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            write_mono(&args[4], &out, sr);
            eprintln!("wrote {} (peak {peak:.3})", args[4]);
        }
        Some("calibrate") if args.len() >= 7 => {
            return calibrate(&args[2], &args[3], &args[4], &args[5], &args[6]);
        }
        Some("render") if args.len() >= 5 => {
            let rate: u32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(48_000);
            let block: usize = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(256);
            let overrides: Vec<String> = std::env::var("SKY_SET")
                .map(|v| {
                    v.split(',')
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            // "default" is what a freshly inserted plugin plays: the first sound
            // (enforced by the plugin's `defaults_are_the_first_preset` test).
            let params = if args[2] == "default" {
                (tonetinkerer_dsp::presets::PRESETS[0].params)()
            } else {
                match tonetinkerer_dsp::presets::find(&args[2]) {
                    Some(p) => (p.params)(),
                    None => {
                        eprintln!(
                            "unknown preset {:?}; available: {}",
                            args[2],
                            tonetinkerer_dsp::presets::PRESETS
                                .iter()
                                .map(|p| p.name)
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                        return ExitCode::FAILURE;
                    }
                }
            };
            let mut params = params;
            let mut macros = tonetinkerer_dsp::Macros::default();
            for o in &overrides {
                if let Err(e) = set_param(&mut params, &mut macros, o) {
                    eprintln!("{e}");
                    return ExitCode::FAILURE;
                }
            }
            let params = tonetinkerer_dsp::macros::apply(&params, &macros);
            let input = if args[3] == "demo" {
                trainsig::demo_phrase(rate as f32)
            } else {
                let (x, sr) = read_mono(&args[3]);
                if sr != rate {
                    eprintln!("input is {sr} Hz; rendering at {sr} Hz");
                }
                return render_file(&x, sr, block, &params, &args[4]);
            };
            return render_file(&input, rate, block, &params, &args[4]);
        }
        Some("demos") if args.len() >= 3 => {
            std::fs::create_dir_all(&args[2]).ok();
            let di = trainsig::demo_phrase(48_000.0);
            write_mono(&format!("{}/00_dry_di.wav", args[2]), &di, 48_000);
            for (i, p) in tonetinkerer_dsp::presets::PRESETS.iter().enumerate() {
                let file = format!(
                    "{}/{:02}_{}.wav",
                    args[2],
                    i + 1,
                    p.name.to_lowercase().replace(' ', "_")
                );
                if render_file(&di, 48_000, 256, &(p.params)(), &file) != ExitCode::SUCCESS {
                    return ExitCode::FAILURE;
                }
            }
        }
        Some("latency") if args.len() >= 3 => {
            let sr: f32 = args[2].parse().expect("rate");
            println!("{}", tonetinkerer_dsp::Engine::new(sr).latency());
        }
        Some("compare") if args.len() >= 6 => {
            let (a, ra) = read_mono(&args[2]);
            let (b, _) = read_mono(&args[3]);
            let shift: usize = args[4].parse().expect("shift");
            let n = (args[5].parse::<f32>().expect("secs") * ra as f32) as usize;
            let mut err = 0.0f32;
            let mut sig = 0.0f32;
            for i in 0..n.min(a.len()).min(b.len().saturating_sub(shift)) {
                err = err.max((a[i] - b[i + shift]).abs());
                sig = sig.max(b[i + shift].abs());
            }
            println!("max_diff={err:e} peak={sig}");
            if !(err <= 1e-4 * sig.max(1e-3)) {
                return ExitCode::FAILURE;
            }
        }
        Some("check") if args.len() >= 3 => {
            let (v, ch) = read_all(&args[2]);
            let finite = v.iter().all(|x| x.is_finite());
            let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            let rms =
                (v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt();
            let width = if ch == 2 {
                let d: f64 = v.chunks(2).map(|c| ((c[0] - c[1]) as f64).powi(2)).sum();
                (d / (v.len() / 2) as f64).sqrt()
            } else {
                0.0
            };
            println!(
                "finite={finite} peak_db={:.2} rms_db={:.2} side_rms_db={:.2}",
                20.0 * peak.max(1e-9).log10(),
                20.0 * rms.max(1e-12).log10(),
                20.0 * width.max(1e-12).log10()
            );
            if !finite || peak > 1.0 || rms < 1e-4 {
                return ExitCode::FAILURE;
            }
        }
        Some("resynth") if args.len() >= 4 => {
            let text = std::fs::read_to_string(&args[2]).expect("read csv");
            let frames: Vec<trainsig::F0Frame> = text
                .lines()
                .filter_map(|l| {
                    let v: Vec<&str> = l.split(',').collect();
                    Some(trainsig::F0Frame {
                        t: v.first()?.parse().ok()?,
                        f0: v.get(1)?.parse().ok()?,
                        rms_db: v.get(2)?.parse().ok()?,
                        onset: v.get(3)? == &"1",
                    })
                })
                .collect();
            let di = trainsig::resynth(&frames, 48_000.0);
            write_mono(&args[3], &di, 48_000);
            eprintln!("wrote {} ({:.1} s)", args[3], di.len() as f32 / 48_000.0);
        }
        Some("riff") if args.len() >= 5 => {
            // riff <root_midi> <bpm> <out.wav> [seconds]
            let root: f32 = args[2].parse().expect("root midi");
            let bpm: f32 = args[3].parse().expect("bpm");
            let secs: f32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(30.0);
            let di = trainsig::metal_riff(48_000.0, root, bpm, secs);
            write_mono(&args[4], &di, 48_000);
            eprintln!("wrote {} ({secs} s riff, root {root}, {bpm} bpm)", args[4]);
        }
        Some("levels") => {
            // Loudness of every sound at the typical playing level (RMS while
            // playing); every sound should land at -16 dBFS.
            let di = trainsig::voicing_phrase(48_000.0);
            for p in tonetinkerer_dsp::presets::PRESETS {
                let params = (p.params)();
                let mut e = tonetinkerer_dsp::assets::build_engine(48_000.0);
                e.reset(&params);
                let mut l = di.clone();
                let mut r = di.clone();
                for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                    e.process(a, Some(b), &params);
                }
                let rms = (l
                    .iter()
                    .chain(r.iter())
                    .map(|v| (*v as f64).powi(2))
                    .sum::<f64>()
                    / (2 * l.len()) as f64)
                    .sqrt();
                println!(
                    "{}\t{:.2}\t{:.2}",
                    p.name,
                    20.0 * rms.log10(),
                    params.amp_level_db
                );
            }
        }
        Some("gen-irs") if args.len() >= 3 => {
            for spec in &irgen::CABS {
                let ir = irgen::generate(spec, 48_000.0);
                let path = format!("{}/{}.wav", args[2], spec.name);
                write_mono(&path, &ir, 48_000);
                eprintln!("wrote {path} ({} samples)", ir.len());
            }
        }
        _ => {
            eprintln!(
                "usage: tonetinkerer-render gen-train <out.wav> [secs] | refamp <voice> <in> <out>"
            );
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// Run a trained model over the DI with nam-rs (the plugin's inference
/// engine), report its error against the reference circuit, and write
/// name / loudness metadata so the plugin can level-match models.
fn calibrate(model_path: &str, voice: &str, di_path: &str, out_path: &str, name: &str) -> ExitCode {
    let voice = refamp::Voice::parse(voice).expect("voice");
    let nam = nam_rs::NamModel::from_file(model_path).expect("parse .nam");
    let mut model = nam_rs::Model::from_nam(&nam).expect("build model");
    let (di, sr) = read_mono(di_path);
    let target = refamp::render_aligned(voice, sr as f32, &di);
    let mut out = di.clone();
    for chunk in out.chunks_mut(256) {
        model.process_buffer(chunk);
    }
    // Error-to-signal ratio over the validation region (last 9 s), as NAM reports it.
    let v0 = di.len() - 9 * sr as usize;
    let (mut err, mut sig) = (0.0f64, 0.0f64);
    for i in v0..di.len() {
        let d = (out[i] - target[i]) as f64;
        err += d * d;
        sig += (target[i] as f64).powi(2);
    }
    let esr = err / sig.max(1e-20);
    let rms = (out.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / out.len() as f64).sqrt();
    let loudness = 20.0 * rms.max(1e-12).log10();
    eprintln!(
        "{name}: nam-rs ESR vs reference (validation) = {esr:.5}, loudness = {loudness:.2} dBFS"
    );

    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(model_path).unwrap()).unwrap();
    let meta = json
        .as_object_mut()
        .unwrap()
        .entry("metadata")
        .or_insert_with(|| serde_json::json!({}));
    let m = meta.as_object_mut().unwrap();
    m.insert("name".into(), name.into());
    m.insert(
        "modeled_by".into(),
        "ToneTinkerer (virtual capture of an in-house reference circuit)".into(),
    );
    m.insert("gear_type".into(), "amp".into());
    m.insert(
        "tone_type".into(),
        match voice {
            refamp::Voice::Clean => "clean",
            refamp::Voice::Crunch => "crunch",
            refamp::Voice::Lead | refamp::Voice::Metal => "hi_gain",
        }
        .into(),
    );
    m.insert("loudness".into(), loudness.into());
    m.insert("validation_esr".into(), esr.into());
    std::fs::write(out_path, serde_json::to_string(&json).unwrap()).unwrap();
    if esr > 0.05 {
        eprintln!("ESR too high: model does not match the reference closely enough");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Run the full plugin engine over `input`, append 3 s for tails, write stereo float WAV.
fn render_file(
    input: &[f32],
    sr: u32,
    block: usize,
    params: &tonetinkerer_dsp::ChainParams,
    out_path: &str,
) -> ExitCode {
    let mut engine = tonetinkerer_dsp::assets::build_engine(sr as f32);
    engine.reset(params);
    let tail = 3 * sr as usize;
    let mut l: Vec<f32> = input
        .iter()
        .copied()
        .chain(std::iter::repeat_n(0.0, tail))
        .collect();
    let mut r = l.clone();
    let t0 = std::time::Instant::now();
    for (cl, cr) in l.chunks_mut(block.max(1)).zip(r.chunks_mut(block.max(1))) {
        engine.process(cl, Some(cr), params);
    }
    let secs = t0.elapsed().as_secs_f64();
    let audio = l.len() as f64 / sr as f64;
    let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
    let rms = (l.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / l.len() as f64).sqrt();
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sr,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(out_path, spec).expect("create output");
    for (a, b) in l.iter().zip(r.iter()) {
        w.write_sample(*a).unwrap();
        w.write_sample(*b).unwrap();
    }
    w.finalize().unwrap();
    eprintln!(
        "{out_path}: peak {:.2} dBFS, rms {:.1} dBFS, latency {} smp, rendered {:.1} s audio in {:.3} s ({:.0}x real time)",
        20.0 * peak.max(1e-9).log10(),
        20.0 * rms.max(1e-9).log10(),
        engine.latency(),
        audio,
        secs,
        audio / secs
    );
    ExitCode::SUCCESS
}

/// Apply a `name=value` override (env `SKY_SET="a=1,b=2"`) for tuning experiments.
fn set_param(
    p: &mut tonetinkerer_dsp::ChainParams,
    m: &mut tonetinkerer_dsp::Macros,
    kv: &str,
) -> Result<(), String> {
    let (k, v) = kv.split_once('=').ok_or(format!("bad override {kv}"))?;
    let f: f32 = v.parse().map_err(|_| format!("bad value in {kv}"))?;
    let b = f != 0.0;
    match k {
        "input_gain_db" => p.input_gain_db = f,
        "gate" => p.gate_enabled = b,
        "gate_threshold_db" => p.gate_threshold_db = f,
        "sustain_on" => p.comp_enabled = b,
        "sustain" => p.sustain = f,
        "drive_on" => p.drive_enabled = b,
        "drive" => p.drive = f,
        "drive_tone" => p.drive_tone = f,
        "drive_level_db" => p.drive_level_db = f,
        "drive_fat" => p.drive_fat = b,
        "amp_model" => p.amp_model = f as usize,
        "amp_gain_db" => p.amp_gain_db = f,
        "amp_level_db" => p.amp_level_db = f,
        "cab_on" => p.cab_enabled = b,
        "cab_ir" => p.cab_ir = f as usize,
        "low_cut" => p.cab_low_cut = f,
        "high_cut" => p.cab_high_cut = f,
        "bass" => p.eq.bass_db = f,
        "mid" => p.eq.mid_db = f,
        "mid_freq" => p.eq.mid_freq = f,
        "treble" => p.eq.treble_db = f,
        "presence" => p.eq.presence_db = f,
        "body_eq" => p.eq.body_db = f,
        "chorus_on" => p.chorus_enabled = b,
        "chorus_rate" => p.chorus.rate_hz = f,
        "chorus_depth" => p.chorus.depth = f,
        "chorus_mix" => p.chorus.mix = f,
        "guitar" => {
            p.guitar = [
                tonetinkerer_dsp::GuitarType::SingleCoil,
                tonetinkerer_dsp::GuitarType::Humbucker,
                tonetinkerer_dsp::GuitarType::Active,
            ][(f as usize).min(2)]
        }
        "m_gain" => m.gain = f,
        "m_body" => m.body = f,
        "m_bright" => m.bright = f,
        "m_mids" => m.mids = f,
        "m_sustain" => m.sustain = f,
        "m_space" => m.space = f,
        "m_volume" => m.volume_db = f,
        "delay_on" => p.delay_enabled = b,
        "delay_ms" => p.delay.time_ms = f,
        "delay_fb" => p.delay.feedback = f,
        "delay_mix" => p.delay.mix = f,
        "delay_tone" => p.delay.tone = f,
        "delay_duck" => p.delay.duck = f,
        "reverb_on" => p.reverb_enabled = b,
        "reverb_decay" => p.reverb.decay_s = f,
        "reverb_mix" => p.reverb.mix = f,
        "reverb_damping" => p.reverb.damping = f,
        "reverb_predelay" => p.reverb.predelay_ms = f,
        _ => return Err(format!("unknown parameter {k}")),
    }
    Ok(())
}
