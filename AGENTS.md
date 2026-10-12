# Notes for agents and contributors

Read ARCHITECTURE.md first. This file is the working rules.

## Build, test, verify

```bash
cargo test                                   # all unit + integration + RT-safety tests (optimized test profile)
                                             # also renders the editor offscreen -> target/snapshots/*.png (review after UI changes)
cargo xtask bundle tonetinkerer-plugin --release   # -> target/bundled/ToneTinkerer.{vst3,clap}
cargo bench -p tonetinkerer-dsp                 # CPU / deadline / latency table
target/tools/bin/clap-validator validate target/bundled/ToneTinkerer.clap
scripts/build-macos-pkg.sh                   # macOS installer (signs if SIGN_APP / SIGN_PKG are set)
scripts/reaper-verify.sh                     # macOS + REAPER: load, PDC, render == offline (refuses if REAPER is open)
cargo run --release -p tonetinkerer-render -- demos render-out/demos   # listen to every sound
cargo run --release -p tonetinkerer-render -- levels   # loudness of every sound (target -16 dBFS)
```

Run `cargo test` (not `cargo test --release`): the allocation detector in
`tests/rt_safety.rs` needs debug assertions, which the test profile keeps on
while still optimizing. The test asserts this so it can't silently pass.

## Hard rules

1. **Nothing in the audio path allocates, locks, logs, or does I/O.** That is
   `Engine::process` and everything it calls, plus `Plugin::process` and
   `Plugin::reset`. Allocate in constructors (`build_engine`, `NamSlot::new`,
   `CabSlot::new`) and hand objects over through the `rtrb` rings. If you add
   a code path to `process`, extend `tests/rt_safety.rs` to cover it.
2. **Never bypass or weaken a test to make it pass**, and never substitute
   placeholder audio processing. If a test fails, find the cause.
3. **The plugin and the offline renderer share one `Engine`.** Don't add
   behaviour to the plugin crate that the renderer can't reproduce; DSP goes
   in `tonetinkerer-dsp`.
4. **Latency must stay constant for a given sample rate** and equal what is
   reported (`latency_is_reported_correctly`). Bypass paths must be
   delay-matched.
5. **Smoothing lives in the DSP.** New continuous parameters get a
   `Smoothed`; new switches get a `Fade` (or `Trails` for time-based effects).
6. **No LLM / AI runtime in the plugin.** NAM is the only neural network, and
   it is plain inference.
7. **Sounds are "inspired by", never presented as an artist's exact rig or
   recording.** Artist / song names appear only in a sound's `inspired_by`
   line, never in the product name, sound names or UI chrome.
9. **Play first.** New features must not add required configuration. Prefer
   extending a macro or a fix button over adding a knob to the Play screen;
   detailed controls belong in the Advanced view.
10. **Sounds stay level-matched.** After changing or adding a sound run
    `tonetinkerer-render levels` and set its `amp_level_db` so it lands at
    -16 dBFS; `every_preset_produces_sane_audio` enforces a < 2 dB spread.
8. **Reference recordings never enter the repository.** `reference-songs/`
   and all derived analysis are git-ignored; bundled assets must be original
   work (see ASSETS.md).

## Adding a parameter

1. Field on `ChainParams` (+ `Default`, + `tone_fields()` if it affects tone).
2. Use it in `Engine::process_chunk` with smoothing.
3. Plugin: field on `ToneParams` with a **new, never reused** `#[id]`, map it in
   `base_params()` and `apply_chain_params()`.
4. GUI widget in `editor.rs` (Advanced); check `target/snapshots/` after `cargo test`. Only add to `play.rs` if it is a
   plain-language control; consider whether a macro (`macros.rs`) should move it.
5. Renderer override name in `set_param` (`tonetinkerer-render/src/main.rs`).
6. Tests: extend the automation stress test (`tests/chain.rs`) and `rt_safety.rs`.

## Adding or voicing a sound

1. Add a `Preset` in `presets.rs` (genre, optional `inspired_by`, description).
2. Optional measured voicing against a reference recording (kept local in
   `reference-songs/<Genre>/`, git-ignored): Demucs guitar stem ->
   `tools/tone-match/f0.py` -> `tonetinkerer-render resynth` (or `riff` for
   chord-based rhythm parts) -> `tools/tone-match/fit.py` / `tails.py`.
   The synthetic DIs are spectrally corrected against a real DI
   (`testsig::realism`), so fitted settings transfer to real guitars.
3. Level-match (`tonetinkerer-render levels`), then `cargo test`.

## Retraining a bundled model

```bash
cargo build --release -p tonetinkerer-render
tools/nam-train/train.sh lead 100        # needs tools/nam-train/.venv with neural-amp-modeler
target/release/tonetinkerer-render calibrate tools/nam-train/work/out_lead/*/model.nam lead \
  tools/nam-train/work/di.wav assets/models/lead.nam "ToneTinkerer Lead"
cargo test
```

`calibrate` fails if the model's validation ESR against the reference
circuit exceeds 0.05. Training works on Apple-silicon GPUs (MPS) and CUDA
(set `"accelerator": "gpu"` in `tools/nam-train/configs/learning.json`).

## Licences

After changing dependencies, run `scripts/third-party-notices.py` to
regenerate THIRD_PARTY_NOTICES.md and THIRD_PARTY_LICENSES.txt (all release
platforms). Every release download gets its own per-platform copy.

## Windows builds

The code is platform-independent. From WSL2 (or macOS), with MinGW-w64
installed (`apt install mingw-w64` / `brew install mingw-w64`):

```bash
rustup target add x86_64-pc-windows-gnu
scripts/build-windows.sh            # -> target/bundled-windows/ToneTinkerer.{vst3,clap}
x86_64-w64-mingw32-objdump -p target/x86_64-pc-windows-gnu/release/tonetinkerer_plugin.dll | grep "DLL Name"
```

The last line must list only Windows system DLLs (no libgcc / winpthread).
`WINDOWS_TOOLCHAIN=msvc` uses cargo-xwin instead (requires accepting
Microsoft's SDK licence). Plugins built natively inside WSL2 are Linux plugins
and will not load in a Windows DAW. Play through the Windows DAW with the
interface's ASIO driver; WSL2 has no low-latency audio.

## Conventions

- Rust 2024 edition, toolchain pinned in `rust-toolchain.toml`.
- Match surrounding style; comment the *why* of DSP decisions.
- Keep `cargo build` warning-free.
