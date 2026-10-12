ToneTinkerer for macOS (Apple silicon and Intel, macOS 11 or newer)

1. Copy ToneTinkerer.vst3 to  ~/Library/Audio/Plug-Ins/VST3/
   Copy ToneTinkerer.clap to  ~/Library/Audio/Plug-Ins/CLAP/
   (In Finder: Go > Go to Folder... and paste the folder path.)

2. This build is not yet notarized by Apple, so macOS blocks it the first
   time. Open Terminal and paste this one line, then press Enter:

   xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/ToneTinkerer.vst3 ~/Library/Audio/Plug-Ins/CLAP/ToneTinkerer.clap

3. Restart your DAW (or rescan plugins). It appears as "ToneTinkerer"
   by GuitarWiring.

Playing: guitar into your interface's instrument (Hi-Z) input, a track with
that input, record-armed with input monitoring on, ToneTinkerer as its first
effect. 48 kHz and a buffer of 64-128 samples work best. Pick a sound and
play: the input level sets itself during your first seconds of playing.

Free and open source (MIT): https://github.com/Tinkercaster/tonetinkerer
