#!/usr/bin/env bash
# Cross-build the Windows x86_64 VST3 and CLAP from WSL2 / Linux / macOS.
#
# Default: the open-source MinGW-w64 toolchain (no Microsoft SDK needed).
#   macOS:        brew install mingw-w64
#   WSL2/Ubuntu:  sudo apt install mingw-w64
#   rustup target add x86_64-pc-windows-gnu
# The result only imports DLLs that ship with Windows 10/11.
#
# Alternative: WINDOWS_TOOLCHAIN=msvc uses cargo-xwin (`cargo install --locked
# cargo-xwin`), which downloads the Microsoft CRT / Windows SDK and asks you to
# accept Microsoft's licence.
#
# Output: target/bundled-windows/ToneTinkerer.vst3 and ToneTinkerer.clap
# Install (from WSL2):
#   cp -r target/bundled-windows/ToneTinkerer.vst3 "/mnt/c/Program Files/Common Files/VST3/"
#   cp    target/bundled-windows/ToneTinkerer.clap "/mnt/c/Program Files/Common Files/CLAP/"
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
if [[ "${WINDOWS_TOOLCHAIN:-gnu}" == msvc ]]; then
  target=x86_64-pc-windows-msvc
  rustup target add "$target" >/dev/null
  cargo xwin build --release -p tonetinkerer-plugin --target "$target"
else
  target=x86_64-pc-windows-gnu
  rustup target add "$target" >/dev/null
  CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
    cargo build --release -p tonetinkerer-plugin --target "$target"
fi
dll="target/$target/release/tonetinkerer_plugin.dll"
out="target/bundled-windows"
rm -rf "$out"
mkdir -p "$out/ToneTinkerer.vst3/Contents/x86_64-win"
cp "$dll" "$out/ToneTinkerer.vst3/Contents/x86_64-win/ToneTinkerer.vst3"
cp "$dll" "$out/ToneTinkerer.clap"
cp THIRD_PARTY_LICENSES.txt THIRD_PARTY_NOTICES.md "$out/"
echo "Built:"
ls -la "$out" "$out/ToneTinkerer.vst3/Contents/x86_64-win"
