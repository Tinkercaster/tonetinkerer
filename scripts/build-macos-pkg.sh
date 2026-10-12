#!/usr/bin/env bash
# Build a macOS installer package for ToneTinkerer (VST3 + CLAP + licences).
#
#   scripts/build-macos-pkg.sh            # unsigned: fine for testing
#   SIGN_APP="Developer ID Application: …" SIGN_PKG="Developer ID Installer: …" \
#     scripts/build-macos-pkg.sh          # signed, ready for notarization:
#   xcrun notarytool submit target/installer/ToneTinkerer.pkg --keychain-profile <profile> --wait
#   xcrun stapler staple target/installer/ToneTinkerer.pkg
#
# Installs to /Library/Audio/Plug-Ins/{VST3,CLAP} and the licence texts to
# /Library/Application Support/GuitarWiring/ToneTinkerer.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print([p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="tonetinkerer-plugin"][0])')"
cargo xtask bundle tonetinkerer-plugin --release

out="target/installer"
stage="$out/stage"
rm -rf "$out"
mkdir -p "$stage/Plug-Ins/VST3" "$stage/Plug-Ins/CLAP" "$stage/Support"
cp -R target/bundled/ToneTinkerer.vst3 "$stage/Plug-Ins/VST3/"
cp -R target/bundled/ToneTinkerer.clap "$stage/Plug-Ins/CLAP/"
cp THIRD_PARTY_LICENSES.txt THIRD_PARTY_NOTICES.md README.md "$stage/Support/"

if [[ -n "${SIGN_APP:-}" ]]; then
  for b in "$stage/Plug-Ins/VST3/ToneTinkerer.vst3" "$stage/Plug-Ins/CLAP/ToneTinkerer.clap"; do
    codesign --force --timestamp --options runtime --sign "$SIGN_APP" "$b"
  done
fi

pkgbuild --root "$stage/Plug-Ins" --install-location /Library/Audio/Plug-Ins \
  --identifier app.guitarwiring.tonetinkerer.plugins --version "$version" "$out/plugins.pkg"
pkgbuild --root "$stage/Support" --install-location "/Library/Application Support/GuitarWiring/ToneTinkerer" \
  --identifier app.guitarwiring.tonetinkerer.support --version "$version" "$out/support.pkg"

sign_args=()
[[ -n "${SIGN_PKG:-}" ]] && sign_args=(--sign "$SIGN_PKG" --timestamp)
productbuild --identifier app.guitarwiring.tonetinkerer --version "$version" \
  --package "$out/plugins.pkg" --package "$out/support.pkg" "${sign_args[@]}" "$out/ToneTinkerer.pkg"
rm -rf "$stage" "$out/plugins.pkg" "$out/support.pkg"
echo "Built $out/ToneTinkerer.pkg (version $version${SIGN_PKG:+, signed})"
