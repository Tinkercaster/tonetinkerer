# Implementation plan and milestones

Each milestone has testable acceptance criteria. "Verified by" names the
command that proves it. Status reflects the current tree.

## M0. Ecosystem research and dependency check ✅

- nice-plug (RustAudio, Codeberg) chosen as the maintained NIH-plug fork;
  pinned to commit `e60b5db`. VST3 via the permissively licensed `vst3`
  crate, CLAP via `clap-sys`.
- `nam-rs` 0.4: pure-Rust NAM inference, allocation-free `process_buffer`,
  WaveNet / LSTM / A2 support.
- egui 0.36 through `nice-plug-egui` (needs Rust >= 1.95, so the toolchain is
  pinned to 1.99.0 in `rust-toolchain.toml`).
- `fft-convolver` (partitioned convolution), `hound` (WAV), `rtrb` (lock-free
  rings), `rfd` (file dialogs).

## M1. Simplest end-to-end plugin ✅

Acceptance: VST3 + CLAP bundles build, load in REAPER, process audio.
Verified by: `cargo xtask bundle tonetinkerer-plugin --release`, `scripts/reaper-verify.sh`.

## M2. DSP components with unit tests ✅

Gate, wah, compressor, overdrive (4x oversampled), biquads, EQ, stereo delay,
FDN reverb, streaming sinc resampler, parameter smoothers, crossfades.
Acceptance: each module has unit tests for its defining behaviour (filter
responses hit their targets, gate opens and closes, oversampler latency is an
integer, delay echo arrives on time, reverb decays at roughly the RT60, high
feedback stays bounded, ...).
Verified by: `cargo test -p tonetinkerer-dsp --lib`.

## M3. NAM inference and cabinet IRs ✅

- Reference circuits + synthetic DI + NAM trainer produce the three bundled
  models; `tonetinkerer-render calibrate` checks nam-rs output against the
  reference circuit (validation ESR < 0.005 for all three) and writes
  loudness metadata.
- IR loading from WAV (any rate / bit depth / channel count), resampling,
  normalisation; custom `.nam` and IR hot-swap without allocation on the
  audio thread.
Acceptance: models load and match their reference circuits; NaN-free; sample
rate round trip is transparent with constant latency; hot-swap tested.
Verified by: `cargo test -p tonetinkerer-dsp` (`custom_model_and_ir_hot_swap`,
`resample::*`, `cab::*`), `tonetinkerer-render calibrate ...`.

## M4. Full chain, offline rendering and lifecycle tests ✅

Acceptance (all in `crates/tonetinkerer-dsp/tests/chain.rs`):
- silence in -> silence out at 44.1 / 48 / 88.2 / 96 kHz for every preset
- no start-up or model-switch thump
- extreme input (+20 dBFS square, maximum gains) never exceeds 0 dBFS
- NaN / inf input never reaches the output and the chain recovers
- stereo effects are wide; mono-out layout works; Auto input follows the guitar channel
- random host block sizes (1 - 4096) give the same output as fixed blocks
- reported latency equals measured latency at every rate
- random automation every 32 samples stays finite and bounded
- parameter changes are smoothed (no instant steps)
- `tests/rt_safety.rs`: no allocation in `process` for every preset, with model swap and reset
Verified by: `cargo test`.

## M5. Presets and GUI ✅

- 9 factory presets (Singing Lead, Alien Drive, Warm Clean, Ambient Lead,
  4 "inspired by" song tones, Rhythm Crunch). Applied through the host's
  parameter API; the browser shows when a preset has been edited.
- egui editor: pedalboard / amp & cab / effects layout, custom knobs, LED
  switches, meters with an input-level guide zone, model / IR loading.
Acceptance: every preset renders sane, level-matched audio
(`every_preset_produces_sane_audio`); the GUI opens in REAPER.

## M6. Host validation and benchmarks ✅

- `clap-validator`: 34 passed, 0 failed (10 skipped: features not implemented, e.g. note ports).
- REAPER: VST3 and CLAP load, report correct PDC at 44.1 / 48 / 96 kHz, and
  render output identical to the offline renderer.
- `cargo bench -p tonetinkerer-dsp`: ~10 % of one core at 48 kHz / 128 samples (docs/BENCHMARKS.md).

## M7. Reference-guided voicing ✅

From user-supplied reference recordings (kept local, git-ignored):
1. Demucs `htdemucs_6s` isolates the guitar stem.
2. pYIN pitch-tracks it (`tools/tone-match/f0.py`); `tonetinkerer-render resynth`
   plays the same notes, timing, bends and vibrato on the plucked-string model,
   so preset renders and reference share the same source material.
3. 1/3-octave long-term spectra are compared (`analyze.py`); `fit.py` searches
   amp model, cab, drive and EQ (Powell) on renders of the real engine.
4. Phrase-end tails give echo period and decay (`tails.py`).

Findings applied: the original voicing (Lead amp + tube-screamer drive) was
10 - 25 dB short at 200 - 500 Hz and too dark above 5 kHz; the references are
closest with the Crunch amp, the new Fat drive mode, the 2x12 cab and a more
open top. "Ballad Sustain" now matches its reference within ~2 dB in most
1/3-octave bands (overall weighted error 2.3 dB); echo times (~430 / 400 / 510 ms)
and RT60 (~4.5 - 5 s) set from the measurements. Side fixes found along the
way: brighter synthesized IRs, f64 biquad state (f32 low-cut filters at high
sample rates amplified their own rounding noise).

Acceptance: measured spectra of the "inspired by" presets on pitch-matched
sources within a few dB of their references between 300 Hz and 5 kHz.

## M8. Realistic synthetic guitar signal ✅

The plucked-string DI used for voicing was compared with a real recorded
guitar DI: it was too bright above 2 kHz and 10 - 30 dB short in the low end,
which biased every fit towards dark, bass-heavy settings. `testsig::realism`
(a fitted low-pass + peak + two shelves) now corrects all voicing and test
signals (`demo_phrase`, `resynth`, `metal_riff`), so measured settings transfer
to real guitars. The training DI for the NAM models is unchanged.

## M9. ToneTinkerer: play first ✅

- Rebrand: ToneTinkerer by GuitarWiring (guitarwiring.app); new plugin IDs.
- **Macros** (`macros.rs`, pure function shared by plugin and renderer):
  Gain, Body, Bright, Mids, Sustain, Space, Volume. Centre = sound as designed.
  Tests check direction (spectra, tails, crest factor), click-free automation
  and every sound at macro extremes.
- **Fix buttons** on the Play screen nudge macros (undoable, automatable).
- **Guitar setup**: guitar type (Single-coil / Humbucker / Active input offset)
  and "Set up my guitar" (6 s peak capture -> input trim to -9 dBFS peaks).
- **Chorus** (stereo, after the EQ) and a 300 Hz **Body** EQ band.
- **Metal amp model** (fourth NAM capture of an in-house reference circuit,
  ESR 0.013).
- **23 sounds** in six genres with an "inspired by" line, level-matched to
  -16 dBFS on the demo phrase (spread < 0.1 dB).
- **Play screen** (default) + Advanced view; swappable skin
  (`assets/skin/background.png`). Offscreen snapshot tests render every
  screen to `target/snapshots/` (they caught a clipped Volume knob and
  missing arrow glyphs).
- **Measured voicing** for every sound with a reference: Demucs stem ->
  pitch-matched resynthesis (or a synthetic palm-mute riff in the song's key
  for metal rhythm) -> Powell fit on the real engine -> settings applied with
  the low end moderated (all fits pushed bass and body to their limits,
  consistent with bass-guitar bleed in the separated stems). Score =
  weighted RMS of the 1/3-octave-band difference to the reference, 20 s window:

| Sound | Reference | Before | Now |
|---|---|---|---|
| Singing Lead | "Always With Me, Always With You" | 2.4 dB | 2.3 dB |
| Emotive Lead | "Until We Say Goodbye" | 6.2 dB | 5.9 dB |
| Vocal Wah Lead | "Cryin'" | 5.0 dB | 4.8 dB |
| Blues Lead | "Before You Accuse Me" | 5.0 dB | 4.6 dB |
| Blues-Rock Lead | "Crossroads" (1968) | 4.7 dB | 4.3 dB |
| Jazz Clean | "Four On Six" | 11.1 dB | 5.1 dB |
| Smooth Jazz Clean | "Breezin'" | 10.4 dB | 3.6 dB |
| Thrash Rhythm | "Master of Puppets" | 4.4 dB | 4.2 dB |
| Modern Metal | "Laid to Rest" | 5.4 dB | 5.0 dB |
| Heavy Riff | "Some Kind of Monster" | 6.0 dB | 5.7 dB |

  "Minor Swing" was skipped (acoustic guitar; not an amp sound). Sounds without
  a reference were voiced by design. Pitfall found and fixed: parallel fits of
  one song shared temp files and produced wrong scores; `fit.py` now uses
  per-process files.

## Next

- Windows x86_64: cross-built with MinGW-w64 (`scripts/build-windows.sh`;
  exports and imports checked); to be played on a Windows host
- Final artwork for the skin
- Public release: run `scripts/build-macos-pkg.sh` with a Developer ID
  (signing + notarization; the unsigned installer and full licence texts
  are done), a Windows installer
- User preset save / load inside the GUI (hosts already save full state)
- Tempo-synced delay shown in note values on the knob
- Optional higher-capacity models (more training data / epochs)
