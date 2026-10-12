**ToneTinkerer** is a free guitar amp simulator plugin (VST3 + CLAP) by [GuitarWiring](https://guitarwiring.app/): plug in, pick what you play, play.

## Install with one line (recommended)

**Mac or Linux**: open Terminal, paste, press Enter:

```
curl -fsSL https://raw.githubusercontent.com/Tinkercaster/tonetinkerer/main/scripts/install.sh | bash
```

**Windows**: open PowerShell, paste, press Enter (asks once for administrator rights):

```
irm https://raw.githubusercontent.com/Tinkercaster/tonetinkerer/main/scripts/install.ps1 | iex
```

It downloads this release and puts both plugins (VST3 and CLAP) in the right folders. Then restart your DAW (or rescan plugins).

## Or download manually

| Your computer | File |
|---|---|
| Mac (Apple silicon or Intel) | **ToneTinkerer-macOS.zip** |
| Windows 10/11 (64-bit) | **ToneTinkerer-Windows.zip** |
| Linux (x86_64) | **ToneTinkerer-Linux.tar.gz** |

Each contains the VST3 and CLAP plugins and a `README.txt` with the steps.

- **Mac**: copy `ToneTinkerer.vst3` to `~/Library/Audio/Plug-Ins/VST3/` and `ToneTinkerer.clap` to `~/Library/Audio/Plug-Ins/CLAP/`, then run once in Terminal (the plugins are not signed by Apple):
  ```
  xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/ToneTinkerer.vst3 ~/Library/Audio/Plug-Ins/CLAP/ToneTinkerer.clap
  ```
- **Windows**: copy the `ToneTinkerer.vst3` folder to `C:\Program Files\Common Files\VST3\` and `ToneTinkerer.clap` to `C:\Program Files\Common Files\CLAP\`.
- **Linux**: copy `ToneTinkerer.vst3` to `~/.vst3/` and `ToneTinkerer.clap` to `~/.clap/`.

## What's inside

- 23 ready-to-play sounds across Clean, Blues & Jazz, Rock, Metal, Lead and Ambient, level-matched.
- Seven plain-language knobs (Gain, Body, Bright, Mids, Sustain, Space, Volume) and one-tap fixes ("Too thin", "Too muddy", ...).
- Automatic input level and an adaptive noise gate: no setup needed.
- An Advanced view with every detail: gate, compressor, wah, overdrive, four neural amp models (NAM), cabinets, EQ, chorus, delay, reverb. Load your own `.nam` captures and cabinet IRs.

## Known limitations

- The plugins are not signed by Apple or Microsoft. The one-line installers take care of that; for manual installs on a Mac, run the `xattr` line above once.
- The Windows and Linux builds are new: they build and package correctly, but have not been played on real machines yet. Please [open an issue](https://github.com/Tinkercaster/tonetinkerer/issues) if anything misbehaves.

Free and open source under the MIT License.
