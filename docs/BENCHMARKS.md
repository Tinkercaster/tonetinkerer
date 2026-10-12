# DSP benchmarks

`cargo bench -p tonetinkerer-dsp` (`crates/tonetinkerer-dsp/benches/chain.rs`):
20 s of guitar-like signal through the full engine, one thread, release build.
Measured 2026-10-10 on an Apple M4 Pro (macOS, ARM64), all 23 sounds, chorus
included. The machine was also running a DAW and other work, so p99.9 values
are noisier than on an idle machine.

| Rate | Block | CPU (one core) | p99.9 block time vs deadline | Latency |
|---|---|---|---|---|
| 48 kHz | 32 | 16.0 % | 27 % | 12 samples (0.25 ms) |
| 48 kHz | 64 | 11.5 % | 18 % | 12 samples (0.25 ms) |
| 48 kHz | 128 | 9.1 - 10.0 % (all sounds) | 13 - 15 % (typical) | 12 samples (0.25 ms) |
| 48 kHz | 256 | 8.1 % | 11 % | 12 samples (0.25 ms) |
| 48 kHz | 512 | 8.0 % | 10 % | 12 samples (0.25 ms) |
| 44.1 kHz | 128 | 9.5 - 10.3 % | 13 - 14 % (typical) | 54 samples (1.22 ms) |
| 96 kHz | 128 | 12.4 - 13.7 % | 19 - 20 % (typical) | 91 samples (0.95 ms) |

Reading the numbers:

- **CPU** = processing time / audio time. Roughly 10 % of one performance
  core at 48 kHz; the NAM WaveNet is the dominant cost.
- **Worst case.** The benchmark also prints the single slowest block. Those
  maxima occasionally exceed 100 % of the deadline, but they occur at random
  positions that change from run to run (never the first block), which is
  OS preemption of the normal-priority benchmark thread rather than engine
  cost. Hosts run the audio callback on a real-time-priority thread. Run with
  `BENCH_SLOW=1` to print where the slowest blocks occurred.
- **Latency** is 12 samples at 48 kHz (the overdrive's linear-phase
  oversampler). At other rates the NAM sample-rate round trip adds its
  filter delay; latency is constant per rate and reported to the host.
  Run projects at 48 kHz for the lowest latency.
