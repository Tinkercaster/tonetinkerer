//! DSP CPU and latency benchmark: `cargo bench -p tonetinkerer-dsp`
//!
//! For each sample rate, block size and preset, processes 20 s of a plucked
//! guitar-like signal through the full engine on one thread and reports:
//! mean CPU load (processing time / audio time), worst block time relative to
//! the block's real-time deadline (p99.9 and max), and the plugin latency.

use std::time::Instant;
use tonetinkerer_dsp::assets::build_engine;
use tonetinkerer_dsp::presets::PRESETS;

fn signal(sr: f32, secs: f32) -> Vec<f32> {
    let n = (sr * secs) as usize;
    let mut v = vec![0.0f32; n];
    let period = (sr * 0.5) as usize;
    for (i, x) in v.iter_mut().enumerate() {
        let t = (i % period) as f32 / sr;
        let f = [196.0, 247.0, 330.0, 392.0][(i / period) % 4];
        let env = (-t * 3.0).exp();
        *x = 0.3
            * env
            * ((2.0 * std::f32::consts::PI * f * t).sin()
                + 0.4 * (4.0 * std::f32::consts::PI * f * t).sin());
    }
    v
}

fn main() {
    let secs = 20.0;
    println!(
        "{:>7} {:>6} {:<16} {:>8} {:>10} {:>10} {:>8}",
        "rate", "block", "preset", "cpu %", "p99.9 %dl", "max %dl", "latency"
    );
    for &sr in &[48_000.0f32, 44_100.0, 96_000.0] {
        let input = signal(sr, secs);
        let blocks: &[usize] = if sr == 48_000.0 {
            &[32, 64, 128, 256, 512]
        } else {
            &[128]
        };
        for &block in blocks {
            let presets: Vec<_> = if block == 128 || sr != 48_000.0 {
                PRESETS.iter().collect()
            } else {
                PRESETS.iter().take(1).collect()
            };
            for preset in presets {
                let p = (preset.params)();
                let mut e = build_engine(sr);
                e.reset(&p);
                let mut l = input.clone();
                let mut r = input.clone();
                let deadline = block as f64 / sr as f64;
                let mut times = Vec::with_capacity(l.len() / block + 1);
                let t0 = Instant::now();
                for (cl, cr) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
                    let t = Instant::now();
                    e.process(cl, Some(cr), &p);
                    times.push(t.elapsed().as_secs_f64());
                }
                let total = t0.elapsed().as_secs_f64();
                // Where do the slowest blocks happen? (index / total blocks)
                let mut idx: Vec<usize> = (0..times.len()).collect();
                idx.sort_by(|&a, &b| times[b].partial_cmp(&times[a]).unwrap());
                let slow: Vec<String> = idx
                    .iter()
                    .take(3)
                    .map(|&i| format!("#{i}:{:.0}%", 100.0 * times[i] / deadline))
                    .collect();
                if std::env::var("BENCH_SLOW").is_ok() {
                    eprintln!(
                        "      slowest blocks of {}: {}",
                        times.len(),
                        slow.join(" ")
                    );
                }
                times.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let p999 = times[(times.len() as f64 * 0.999) as usize - 1];
                let max = *times.last().unwrap();
                println!(
                    "{:>7} {:>6} {:<16} {:>7.2}% {:>9.1}% {:>9.1}% {:>4} smp ({:.2} ms)",
                    sr as u32,
                    block,
                    preset.name,
                    100.0 * total / secs as f64,
                    100.0 * p999 / deadline,
                    100.0 * max / deadline,
                    e.latency(),
                    e.latency() as f64 / sr as f64 * 1000.0
                );
                std::hint::black_box(&l);
            }
        }
    }
}
