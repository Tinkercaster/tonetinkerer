# Bundled assets: provenance

Every file in `assets/` is original work (plus optional user-supplied skin artwork in `assets/skin/`) created for this project, so it can
be distributed with the plugin under the project license.

## Amp models (`assets/models/*.nam`)

| File | Voicing | Validation ESR vs reference circuit |
|---|---|---|
| `clean.nam` | Clean, breaks up when dug in | 0.0008 |
| `crunch.nam` | Cranked British-style crunch | 0.0008 |
| `lead.nam` | High-gain three-stage lead | 0.0032 |
| `metal.nam` | Tight, scooped four-stage modern / thrash high gain | 0.0127 |

How they were made (fully reproducible, see AGENTS.md):

1. `tonetinkerer-render gen-train`: a 240 s synthetic guitar DI signal
   (physically-inspired plucked strings: single notes, bends, vibrato,
   chords, palm mutes across -45..0 dBFS, plus sweeps and noise), seeded and
   deterministic.
2. `tonetinkerer-render refamp <voice>`: the DI rendered through an in-house
   analytic tube-amplifier circuit model (`crates/tonetinkerer-render/src/refamp.rs`).
3. `tools/nam-train/train.sh`: the official NAM trainer (`neural-amp-modeler`,
   standard WaveNet) learns DI -> reference-circuit output.
4. `tonetinkerer-render calibrate`: verifies with nam-rs (the plugin's inference
   engine) and writes name / loudness metadata.

They are "virtual captures": neural models of our own circuits, not
captures of any commercial amplifier.

## Cabinet IRs (`assets/irs/*.wav`)

Synthesized by `tonetinkerer-render gen-irs` (`src/irgen.rs`): designed
magnitude responses of close-miked guitar speakers (cabinet resonance,
cone break-up, high-frequency roll-off), made minimum-phase via the real
cepstrum, plus cabinet / room reflections. Not measurements of any product.

## Not bundled

User-supplied `.nam` captures and IR files are loaded at runtime from the
user's disk and never copied into the plugin. Reference recordings used for
voicing analysis (`reference-songs/`) are git-ignored and never distributed;
nothing derived from them is bundled, only preset parameter values.

## Skin artwork (`assets/skin/background.png`)

Original painting made for this project with OpenAI's image model through
Codex (the GuitarWiring `brand-art` workflow): a painterly stage scene in
violet / cyan light. No third-party artwork, text or logos. Drawn behind the UI
with a light veil (see `crates/tonetinkerer-plugin/src/skin.rs`).

## Fonts (`assets/fonts/`)

Inter by Rasmus Andersson (SIL Open Font License 1.1, licence alongside), unmodified.
