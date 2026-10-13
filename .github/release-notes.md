ToneTinkerer is a free guitar amp simulator plugin (VST3 and CLAP) by [GuitarWiring](https://guitarwiring.app/). Plug in, pick what you play, and play.

## Install with one line

Mac or Linux: open Terminal, paste this and press Enter.

```
curl -fsSL https://raw.githubusercontent.com/Tinkercaster/tonetinkerer/main/scripts/install.sh | bash
```

Windows: open PowerShell, paste this and press Enter. Windows asks once for administrator rights.

```
irm https://raw.githubusercontent.com/Tinkercaster/tonetinkerer/main/scripts/install.ps1 | iex
```

It downloads this release and puts both plugins (VST3 and CLAP) in the right folders. Then restart your DAW (or rescan plugins).

## Download it yourself

| Your computer | File |
|---|---|
| Mac (Apple silicon or Intel) | ToneTinkerer-macOS.zip |
| Windows 10/11 (64-bit) | ToneTinkerer-Windows.zip |
| Linux (x86_64) | ToneTinkerer-Linux.tar.gz |

Each contains the VST3 and CLAP plugins and a `README.txt` with the steps.

- Mac: copy `ToneTinkerer.vst3` to `~/Library/Audio/Plug-Ins/VST3/` and `ToneTinkerer.clap` to `~/Library/Audio/Plug-Ins/CLAP/`. The plugins are not signed by Apple, so run this once in Terminal:
  ```
  xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/ToneTinkerer.vst3 ~/Library/Audio/Plug-Ins/CLAP/ToneTinkerer.clap
  ```
- Windows: copy the `ToneTinkerer.vst3` folder to `C:\Program Files\Common Files\VST3\` and `ToneTinkerer.clap` to `C:\Program Files\Common Files\CLAP\`.
- Linux: copy `ToneTinkerer.vst3` to `~/.vst3/` and `ToneTinkerer.clap` to `~/.clap/`.

## What's inside

23 level-matched sounds across Clean, Blues & Jazz, Rock, Metal, Lead and Ambient. Seven knobs with plain names (Gain, Body, Bright, Mids, Sustain, Space, Volume) and one-tap fixes such as "Too thin" and "Too muddy". The noise gate measures your input's hiss in pauses and sets itself.

The Advanced view has every setting: gate, compressor, wah, overdrive, four neural amp models (NAM), five cabinets (two of them real miked 4x12s, CC0 recordings by Jester Dyne Productions), EQ, chorus, delay and reverb. You can load your own `.nam` captures and cabinet IRs.

## Known limitations

- The plugins are not signed by Apple or Microsoft. The one-line installers handle that. If you install by hand on a Mac, run the `xattr` line above once.
- The Windows and Linux builds compile and package correctly, but nobody has played through them on a real machine yet. Please [open an issue](https://github.com/Tinkercaster/tonetinkerer/issues) if anything misbehaves.

Free and open source under the MIT License.
