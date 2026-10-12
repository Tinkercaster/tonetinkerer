# ToneTinkerer

**Plug in, pick what you play, play.** A free guitar amp simulator plugin
(VST3 + CLAP) by [GuitarWiring](https://guitarwiring.app/): *we tinker so you
don't have to.*

![ToneTinkerer: pick a sound, turn a few plain-language knobs, play](docs/images/simple.png)

ToneTinkerer is built around one idea: play first, and only configure if
something is missing.

- **23 ready-to-play sounds** across Clean, Blues & Jazz, Rock, Metal, Lead and
  Ambient. Each one is a complete rig (pedals, amp, cabinet, effects), level-matched
  so switching sounds never jumps in volume.
- **Seven plain-language knobs** instead of dozens: Gain, Body, Bright, Mids,
  Sustain, Space, Volume. The centre is the sound as designed; each knob moves
  several hidden settings at once.
- **One-tap fixes** for what you hear: *Too thin, Too muddy, Too harsh, Too dull,
  Too boxy, Gets lost in the mix, Needs more sustain, More dirt, Cleaner, Too
  washy, Too dry.*
- **No level setup**: the plugin learns how hot your guitar arrives during
  your first seconds of playing and sets the input for you, so every sound
  (clean or high gain) plays at the same volume on any interface. A noise
  gate learns your input's own hiss and closes just above it.
- **Advanced view** with every detail (noise gate, compressor, wah, overdrive,
  four neural amp models, cabinet IRs, EQ, chorus, delay, reverb) for people who
  like to tinker.

<details>
<summary><b>Advanced view</b>: every detail of the rig, for people who like to tinker</summary>

![ToneTinkerer Advanced view: gate, compressor, wah, overdrive, neural amp, cabinet, chorus, delay, reverb](docs/images/advanced.png)

</details>

Under the hood: Neural Amp Modeler inference ([nam-rs](https://crates.io/crates/nam-rs)),
partitioned-convolution cabinets, [nice-plug](https://codeberg.org/RustAudio/nice-plug)
and egui, all in Rust. Latency at 48 kHz is 12 samples (0.25 ms).

## The sounds

| Genre | Sound | Inspired by |
|---|---|---|
| Clean | Studio Clean | |
| | Funk Clean | classic funk and disco rhythm guitar |
| | 80s Chorus Clean | 1980s pop and rock clean tones |
| Blues & Jazz | Jazz Clean | Wes Montgomery, "Four On Six" |
| | Smooth Jazz Clean | George Benson, "Breezin'" |
| | Blues Breakup | classic electric blues (B.B. King style) |
| | Blues Lead | Eric Clapton, "Before You Accuse Me" |
| | Blues-Rock Lead | Cream, "Crossroads" (1968) |
| Rock | Classic Crunch | classic 70s British rock rhythm |
| | Hard Rock | 80s hard rock rhythm and lead |
| | Rhythm Crunch | |
| Metal | Thrash Rhythm | Metallica, "Master of Puppets" |
| | Modern Metal | Lamb of God, "Laid to Rest" |
| | Heavy Riff | Metallica, "Some Kind of Monster" |
| | Metal Lead | |
| Lead | Singing Lead | Joe Satriani, "Always With Me, Always With You" |
| | Emotive Lead | Joe Satriani, "Until We Say Goodbye" |
| | Vocal Wah Lead | Joe Satriani, "Cryin'" |
| | Wah Shred | Joe Satriani, "Surfing with the Alien" |
| | Smooth Lead, High-Gain Lead | |
| Ambient | Ambient Lead, Dreamy Swells | |

"Inspired by" describes the character a sound aims for. Sounds are original
settings of this plugin, not reproductions of any artist's rig or recording.
Sounds with a song reference were voiced by measuring the guitar in that
recording (see IMPLEMENTATION_PLAN.md); the recordings themselves are never
part of this project.

## Download and install

**Easiest: one line.** It downloads the latest release and puts both plugins
(VST3 and CLAP) in the right folders.

- **Mac or Linux**: open Terminal, paste, press Enter:
  ```bash
  curl -fsSL https://raw.githubusercontent.com/Tinkercaster/tonetinkerer/main/scripts/install.sh | bash
  ```
- **Windows**: open PowerShell, paste, press Enter (Windows asks once for
  administrator rights, because plugins live in Program Files):
  ```powershell
  irm https://raw.githubusercontent.com/Tinkercaster/tonetinkerer/main/scripts/install.ps1 | iex
  ```

Then restart your DAW (or rescan plugins). In REAPER it appears as
"VST3: ToneTinkerer (GuitarWiring)" and "CLAP: ToneTinkerer".

**Or download it yourself** from
**[Releases](https://github.com/Tinkercaster/tonetinkerer/releases/latest)**.
Each download has both plugins and a `README.txt` with the steps.

| Your computer | Download | Copy the plugins to |
|---|---|---|
| Mac (Apple silicon or Intel) | `ToneTinkerer-macOS.zip` | `~/Library/Audio/Plug-Ins/VST3/` and `~/Library/Audio/Plug-Ins/CLAP/` |
| Windows 10/11 (64-bit) | `ToneTinkerer-Windows.zip` | `C:\Program Files\Common Files\VST3\` and `...\CLAP\` |
| Linux (x86_64) | `ToneTinkerer-Linux.tar.gz` | `~/.vst3/` and `~/.clap/` |

The plugins are not signed by Apple, so on a Mac a manually downloaded copy
needs this once in Terminal (the one-line installer above doesn't):

```bash
xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/ToneTinkerer.vst3 ~/Library/Audio/Plug-Ins/CLAP/ToneTinkerer.clap
```

### Build it yourself

```bash
cargo xtask bundle tonetinkerer-plugin --release          # or bundle-universal on macOS
cp -R target/bundled/ToneTinkerer.vst3 ~/Library/Audio/Plug-Ins/VST3/
cp -R target/bundled/ToneTinkerer.clap ~/Library/Audio/Plug-Ins/CLAP/
```

Or build a macOS installer: `scripts/build-macos-pkg.sh` -> `target/installer/ToneTinkerer.pkg`
(unsigned unless `SIGN_APP` / `SIGN_PKG` name your Developer ID certificates;
notarization steps are in the script). Releases are built by
[.github/workflows/release.yml](.github/workflows/release.yml) whenever a
`v*` tag is pushed.

## Playing through it

Works with any audio interface:

1. Guitar into the interface's instrument (Hi-Z) input.
2. In the DAW: a track with that input, record-armed, input monitoring on,
   ToneTinkerer as its first effect. Use **48 kHz** and a buffer of 64-128 samples.
3. Pick a sound and play. The input level sets itself during your first
   seconds of playing. If something is missing, turn a knob or tap a fix.

To use one device for input and another for output on macOS (e.g. an
interface in, laptop speakers out), create an Aggregate Device in Audio MIDI
Setup and select it in the DAW.

## Your own amps, cabinets and artwork

- **Custom amp**: Advanced → "Load .nam…" loads any NAM capture.
- **Custom cabinet**: Advanced → "Load IR…" loads any impulse-response `.wav`.
- **Skin**: put artwork at `assets/skin/background.png` (1120x700 or larger)
  and rebuild; it is drawn behind the interface.

## Windows (including WSL2 development)

Cross-build the Windows plugin from WSL2 or macOS with the open-source
MinGW-w64 toolchain (`sudo apt install mingw-w64` / `brew install mingw-w64`):

```bash
rustup target add x86_64-pc-windows-gnu
scripts/build-windows.sh            # -> target/bundled-windows/ToneTinkerer.{vst3,clap}
```

Copy the results to `C:\Program Files\Common Files\VST3` and `...\CLAP`. A plugin
compiled natively inside WSL2 is a Linux plugin and won't load in a Windows DAW,
so always use the cross-build. The Windows build is verified to compile, export
the VST3 and CLAP entry points and import only DLLs that ship with Windows
10/11; it has not yet been played on a Windows machine.

## Development

See [ARCHITECTURE.md](ARCHITECTURE.md), [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md),
[AGENTS.md](AGENTS.md), [ASSETS.md](ASSETS.md), [docs/BENCHMARKS.md](docs/BENCHMARKS.md)
and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). `cargo test` also writes
screenshots of every editor screen to `target/snapshots/`.

```bash
cargo test                                            # DSP, macros, offline-render, lifecycle, real-time safety
cargo bench -p tonetinkerer-dsp                       # CPU and latency
cargo run --release -p tonetinkerer-render -- demos render-out/demos   # render every sound
cargo run --release -p tonetinkerer-render -- levels  # loudness of every sound
cargo run --release -p tonetinkerer-plugin --features standalone --bin tonetinkerer-standalone -- -b dummy  # GUI without a DAW
```

## License

ToneTinkerer is open source under the [MIT License](LICENSE). Third-party
components keep their own licences (all permissive; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)); the bundled Inter font is
under the SIL Open Font License 1.1.
