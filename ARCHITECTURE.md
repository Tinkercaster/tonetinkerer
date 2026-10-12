# ToneTinkerer architecture

ToneTinkerer is a play-first guitar amplifier simulator plugin (VST3 + CLAP):
a library of complete, level-matched sounds across genres, adjusted through a
few plain-language macro controls. The amplifier is Neural Amp Modeler (NAM)
inference; everything around it is classic DSP.

## Control model

```
Sound (preset)  ──►  detailed ChainParams  ──►  macros::apply(base, Macros)  ──►  Engine
 (Play browser)       (Advanced view)            (Gain Body Bright Mids                (DSP)
                                                  Sustain Space Volume)
```

- A **sound** (`presets.rs`) is a complete set of detailed parameters, with
  a genre and an optional "inspired by" line.
- **Macros** (`macros.rs`) are relative offsets: 0 = the sound as designed,
  -1/+1 the extremes. `apply` is a pure function (no allocation), called in
  `ToneParams::chain_params` on the audio thread and identically by the
  renderer, so the macros are automatable and testable offline. Their measured
  effects are verified in `tests/macros.rs`.
- **Fix buttons** on the Play screen only nudge macros (one host gesture each).
- **Input level**: there is no automatic input gain. Sounds are level-matched
  at a typical playing level (`testsig::VOICING_LEVEL_DB`, peaks around -18
  dBFS) so clean sounds sit with distorted ones without boosting the input
  (an automatic input gain that did so raised the interface's hiss by up to
  30 dB in front of every amp and was removed). Input Trim is manual. The old
  Guitar (pickup type) parameter is kept for saved projects but no longer used.
- The "edited" marker compares the detailed parameters (not the macros) with
  the selected sound.

## Signal chain

```
guitar in ─► input select / trim ─► noise gate ─► wah ─► compressor
          ─► overdrive (4x oversampled) ─► NAM amp ─► cabinet IR ─► EQ
          ─► chorus ─► stereo delay ─► reverb ─► output level ─► safety limiter ─► out
```

Mono from the input selector through the EQ; stereo from the chorus onwards.

| Stage | Implementation | Notes |
|---|---|---|
| Input select | `chain::Engine::select_input` | Auto / Left / Right. Auto follows the input something is played on (any interface, guitar on input 1 or 2): each input learns its own hiss level and counts as played only well above it, so an empty, hissy input is never mixed in (before the first note the quieter input is used; in pauses the choice holds). It takes one channel when both are identical, blends two instruments at similar levels, and switches instantly onto the only live channel, so a channel dropping out never dips the level |
| Noise gate | `gate.rs` | RMS-style level (10 ms), 5 dB hysteresis, 40 ms hold, fast (10 ms) close. Threshold = the sound's, or 9 dB above the input's measured hiss (closing ~4 dB above it, so notes ring until nearly gone), whichever is higher. The hiss is learned only while the input sounds like hiss (top band vs low band power; never by zero crossings, which hiss adds to a quiet note), never while a note rings |
| Wah | `wah.rs` | TPT state-variable filter 350 Hz - 2.3 kHz; Manual (automate it like an expression pedal) or Auto (envelope) |
| Sustain | `compressor.rs` | Feed-forward, soft knee, program-dependent (opto-like) release, auto make-up |
| Overdrive | `overdrive.rs` | Mid-hump clipping (720 Hz or 250 Hz "Fat" high-pass on the clipping branch), soft asymmetric diode clipper, tone low-pass; 4x polyphase FIR oversampling with an integer 12-sample latency that the bypass path matches |
| Amp | `amp.rs` + `nam-rs` | WaveNet/LSTM inference, loudness normalisation to -18 dBFS, per-slot sample-rate round trip, silent post-reset warm-up, warm-up-then-crossfade model switching |
| Cabinet | `cab.rs` + `fft-convolver` | Zero-latency uniformly partitioned FFT convolution, IR resampling + normalisation at load, 24 dB/oct low / high cuts |
| EQ | `eq.rs` | Bass shelf, 300 Hz body peak, sweepable mid peak, treble shelf, presence peak (RBJ biquads, f64 state) |
| Chorus | `chorus.rs` | One line, two quadrature-modulated Hermite taps (L/R, ~6-14 ms), bass kept out of the modulated path |
| Delay | `delay.rs` | Two Hermite-interpolated modulated lines, band-limited saturating feedback, ping-pong, ducking, tape-like time glide |
| Reverb | `reverb.rs` | Pre-delay, 4 series all-pass diffusers, 8-line Householder FDN with damping and modulation, RT60 control |
| Output | `chain.rs` | Smoothed level + soft-knee ceiling (transparent below -0.9 dBFS, never above 0 dBFS) |

Delay and reverb have *trails*: bypassing them fades their input, lets the
tail ring out, and only stops processing once it has decayed.

## Workspace

```
crates/tonetinkerer-dsp      all DSP, sounds (presets.rs), macros, bundled assets,
                             testsig.rs (synthetic DI signals for tests/voicing). No plugin dependencies.
crates/tonetinkerer-plugin   nice-plug VST3/CLAP wrapper, parameters, editor:
                             play.rs (default screen), editor.rs (Advanced), widgets.rs, skin.rs, setup.rs
crates/tonetinkerer-render   offline renderer + dev tools (training data, IRs, riffs, analysis, levels)
xtask                        nice-plug bundler (`cargo xtask bundle tonetinkerer-plugin --release`)
assets/models                bundled .nam models (Clean, Crunch, Lead, Metal)
assets/skin                  optional background artwork (picked up at build time)
assets/irs                bundled cabinet IRs
tools/nam-train           NAM training scripts (Python venv, git-ignored work dir)
tools/tone-match          reference-recording analysis / preset fitting (dev only)
tests/reaper              ReaScript used by scripts/reaper-verify.sh
```

The plugin and the offline renderer run exactly the same `Engine`; the
renderer is how every automated audio test works, and REAPER's render is
verified to match it sample for sample.

## Real-time design

The audio callback (`Plugin::process` -> `Engine::process`) never allocates,
locks, logs or touches the disk. This is enforced by `tests/rt_safety.rs`
(`assert_no_alloc` around every preset at several sample rates, including a
model hot-swap and a reset).

- **Allocation happens in constructors only.** `assets::build_engine` and
  `NamSlot::new` / `CabSlot::new` allocate; they run in `activate()` (main
  thread) or the background task thread.
- **Hand-over is lock-free.** Loaded custom models / IRs travel to the audio
  thread through an `rtrb` SPSC ring (`ToAudio`), tagged with an engine
  *generation* so slots built for an old sample rate are rejected.
- **Freeing happens elsewhere.** Replaced slots go back through a second ring
  (`Trash`) and are dropped by the GUI thread or the loader.
- **Variable block sizes.** `Engine::process` accepts any host block size and
  works internally in chunks of at most 256 samples; output is identical
  whatever the host block size (tested with random sizes 1 - 4096).
- **Sample-rate changes.** `activate()` rebuilds the whole engine for the new
  rate (models re-prepared, IRs resampled, filters redesigned) and reports
  the new latency to the host.
- **Host restarts.** Hosts also deactivate/re-activate at the same rate for
  bookkeeping (REAPER does this to CLAP plugins when switching to offline
  render, sometimes mid-render). A re-activation within a second at the same
  rate resumes the parked engine; if the transport was playing, the reset
  that follows is skipped too, so the restart is inaudible. Stopped restarts
  reset, and `Engine::reset` sounds exactly like a fresh engine
  (`reset_sounds_like_a_fresh_engine`).
- **NaN containment.** Inputs are sanitised; every recursive filter and
  feedback network resets itself if it ever produces a non-finite value; the
  output is sanitised again before the limiter.

## Parameters, smoothing and automation

All 55 controls (7 macros, guitar type and 47 detailed settings) are host
parameters (automatable, saved with the project).
`SAMPLE_ACCURATE_AUTOMATION` is on, so nice-plug splits blocks at parameter
changes. Smoothing lives in the DSP (`util::Smoothed`, per-sample one-pole;
`util::Fade` for switches), not in nice-plug, so offline renders behave
exactly like the plugin. Filter coefficients follow smoothed values every 16
samples while they move. Presets are applied through the host's parameter
API (begin / set / end), so they are undoable and recordable. Custom model
and IR paths are `#[persist]` fields restored with the project.

## Latency

| Host rate | Latency |
|---|---|
| 48 kHz | 12 samples (0.25 ms), the overdrive oversampler only |
| other rates | 12 + the NAM sample-rate round trip (e.g. 44.1 kHz: see `tonetinkerer-render latency 44100`) |

NAM captures are only valid at their capture rate (48 kHz for all bundled
models), so at other host rates the amp stage runs a windowed-sinc
host -> 48 kHz -> host round trip with a constant, measured latency. All model
slots are padded to the same latency so switching never shifts timing.
Latency is constant for a given sample rate (bypassing anything does not
change it) and is reported to the host.

## Bundled models and IRs

See [ASSETS.md](ASSETS.md). In short: the four NAM models are *virtual
captures* trained (with the official NAM trainer) on in-house reference
circuits (`tonetinkerer-render/src/refamp.rs`) using a synthetic DI signal, and
the IRs are synthesized minimum-phase speaker responses. Everything is
original work, so there are no third-party licensing questions. Users can
load any `.nam` capture and any IR `.wav` into the Custom slots.

## Key decisions and trade-offs

| Decision | Why | Trade-off |
|---|---|---|
| nice-plug (RustAudio fork of NIH-plug) pinned to a git commit | The maintained fork; not on crates.io yet | Updates are manual |
| `nam-rs` for inference | Pure Rust, allocation-free, parity-tested against NAM Core | Single-threaded per model |
| Train our own models | License-clean, reproducible, voiced for the target style | Not captures of real amps (users can load those) |
| Synthesized IRs | License-clean | Not measured cabinets (users can load those) |
| DSP-side smoothing | Offline render == plugin output | Duplicates what nice-plug smoothers could do |
| Overdrive always in the chain (bypass delay-matched) | Constant latency, click-free bypass | 0.25 ms latency even when the drive is off |
| Resample around NAM at non-48 kHz rates | Correct model behaviour at every rate | Small extra latency away from 48 kHz |
| egui via nice-plug-egui (OpenGL) | Native, simple, custom widgets | Fixed-size window |
| Macros as a pure function over the sound's settings | Automatable, identical offline, no state duplication | Macro extremes clamp at parameter limits |
| Sounds level-matched on a realistic phrase | Switching sounds never jumps in level | Clean vs. saturated loudness still depends on how hard you play |
| Voicing with a spectrally corrected synthetic DI | Measured against a real guitar DI so fits don't encode synthesizer artefacts | Real playing still varies by guitar and pickups (Body / Bright macros) |
| Delay/reverb trails | Musical bypass | A little CPU after bypass until the tail decays |

## Platforms

macOS ARM64 is the primary, tested target. The code is portable; Windows
x86_64 builds are produced by cross-compiling with `cargo xwin` (from WSL2
or macOS). See [README.md](README.md).
