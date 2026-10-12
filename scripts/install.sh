#!/usr/bin/env bash
# One-line installer for macOS and Linux:
#
#   curl -fsSL https://raw.githubusercontent.com/Tinkercaster/tonetinkerer/main/scripts/install.sh | bash
#
# Downloads the latest release and installs the VST3 and CLAP plugins for the
# current user. Files fetched with curl are not quarantined by macOS, so the
# unsigned plugins load without any extra step.
#
# Options (environment variables):
#   TONETINKERER_VERSION=v0.1.0   install that release instead of the latest
#   VST3_DIR=... CLAP_DIR=...     install somewhere else
set -euo pipefail

repo="Tinkercaster/tonetinkerer"
version="${TONETINKERER_VERSION:-latest}"

case "$(uname -s)" in
  Darwin)
    asset="ToneTinkerer-macOS.zip"
    vst3_dir="${VST3_DIR:-$HOME/Library/Audio/Plug-Ins/VST3}"
    clap_dir="${CLAP_DIR:-$HOME/Library/Audio/Plug-Ins/CLAP}"
    ;;
  Linux)
    [ "$(uname -m)" = "x86_64" ] || { echo "Sorry: ToneTinkerer for Linux is built for x86_64 only." >&2; exit 1; }
    asset="ToneTinkerer-Linux.tar.gz"
    vst3_dir="${VST3_DIR:-$HOME/.vst3}"
    clap_dir="${CLAP_DIR:-$HOME/.clap}"
    ;;
  *)
    echo "This installer is for macOS and Linux. On Windows, see the README." >&2
    exit 1
    ;;
esac

if [ "$version" = latest ]; then
  url="https://github.com/$repo/releases/latest/download/$asset"
else
  url="https://github.com/$repo/releases/download/$version/$asset"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading ToneTinkerer ($version)..."
curl -fL --progress-bar -o "$tmp/$asset" "$url"

echo "Unpacking..."
case "$asset" in
  *.zip) ditto -x -k "$tmp/$asset" "$tmp/x" ;;
  *.tar.gz) mkdir -p "$tmp/x" && tar xzf "$tmp/$asset" -C "$tmp/x" ;;
esac
src="$(find "$tmp/x" -maxdepth 1 -type d -name 'ToneTinkerer-*' | head -n 1)"
[ -d "$src/ToneTinkerer.vst3" ] && [ -e "$src/ToneTinkerer.clap" ] || {
  echo "The download did not contain the plugins. Please report this at https://github.com/$repo/issues" >&2
  exit 1
}

mkdir -p "$vst3_dir" "$clap_dir"
rm -rf "$vst3_dir/ToneTinkerer.vst3" "$clap_dir/ToneTinkerer.clap"
cp -R "$src/ToneTinkerer.vst3" "$vst3_dir/"
cp -R "$src/ToneTinkerer.clap" "$clap_dir/"
if [ "$(uname -s)" = Darwin ]; then
  # Harmless if absent; covers copies that arrived some other way.
  xattr -dr com.apple.quarantine "$vst3_dir/ToneTinkerer.vst3" "$clap_dir/ToneTinkerer.clap" 2>/dev/null || true
fi

echo
echo "Installed:"
echo "  $vst3_dir/ToneTinkerer.vst3"
echo "  $clap_dir/ToneTinkerer.clap"
echo
echo "Restart your DAW (or rescan plugins) and look for ToneTinkerer by GuitarWiring."
