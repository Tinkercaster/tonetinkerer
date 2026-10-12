ToneTinkerer for Windows (64-bit, Windows 10 or 11)

1. Copy the ToneTinkerer.vst3 folder to  C:\Program Files\Common Files\VST3\
   Copy ToneTinkerer.clap to             C:\Program Files\Common Files\CLAP\
   (Create the CLAP folder if it does not exist.)

2. Restart your DAW (or rescan plugins). It appears as "ToneTinkerer"
   by GuitarWiring.

Playing: guitar into your interface's instrument (Hi-Z) input and use the
interface's ASIO driver in your DAW. A track with that input, record-armed
with input monitoring on, ToneTinkerer as its first effect. 48 kHz and a
buffer of 64-128 samples work best. Pick a sound and play: the input level
sets itself during your first seconds of playing.

Next time, the easiest way to install or update is one line
(Terminal on Mac/Linux, PowerShell on Windows):

   irm https://raw.githubusercontent.com/Tinkercaster/tonetinkerer/main/scripts/install.ps1 | iex

Free and open source (MIT): https://github.com/Tinkercaster/tonetinkerer
