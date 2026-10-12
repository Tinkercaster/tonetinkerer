#!/usr/bin/env bash
# Verify that ToneTinkerer loads and processes audio in REAPER (macOS).
#
# Refuses to run while REAPER is already open (it never touches a session
# you are working in) and only ever stops the REAPER instance it started.
#
#  1. bundles + installs the VST3 and CLAP into ~/Library/Audio/Plug-Ins
#  2. per format / sample rate: launches REAPER with a throwaway config,
#     inserts the plugin on a track with a DI clip, renders, and checks the
#     plugin was found, PDC equals the plugin's latency, and the output is
#     finite, non-silent, <= 0 dBFS
#  3. at 48 kHz: REAPER's render must match the offline renderer
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
REAPER="${REAPER:-/Applications/REAPER.app/Contents/MacOS/REAPER}"
if pgrep -f "REAPER.app/Contents/MacOS/REAPER" >/dev/null; then
  echo "REAPER is already running. Close it first (this script will not touch your session)." >&2
  exit 2
fi
work="$root/target/reaper-verify"
rm -rf "$work"; mkdir -p "$work"

cargo xtask bundle tonetinkerer-plugin --release >/dev/null
cargo build --release -q -p tonetinkerer-render
R="$root/target/release/tonetinkerer-render"
mkdir -p ~/Library/Audio/Plug-Ins/VST3 ~/Library/Audio/Plug-Ins/CLAP
rm -rf ~/Library/Audio/Plug-Ins/VST3/ToneTinkerer.vst3 ~/Library/Audio/Plug-Ins/CLAP/ToneTinkerer.clap
cp -R target/bundled/ToneTinkerer.vst3 ~/Library/Audio/Plug-Ins/VST3/
cp -R target/bundled/ToneTinkerer.clap ~/Library/Audio/Plug-Ins/CLAP/

"$R" demos "$work/demos" 2>/dev/null
di="$work/demos/00_dry_di.wav"
"$R" render default "$di" "$work/offline_default.wav" 48000 512 2>/dev/null

fail=0
run_case() {
  local fx="$1" rate="$2" tag="$3"
  local dir="$work/$tag"; mkdir -p "$dir/out"; : > "$dir/reaper.ini"
  # The throwaway config is unlicensed, so REAPER shows its evaluation dialog
  # on launch: click it to continue (each case waits up to two minutes).
  SKY_DI="$di" SKY_OUT_DIR="$dir/out" SKY_LOG="$dir/log.txt" SKY_FX="$fx" SKY_SRATE="$rate" \
    "$REAPER" -nosplash -cfgfile "$dir/reaper.ini" "$root/tests/reaper/render_test.lua" >"$dir/stdout.txt" 2>&1 &
  local pid=$!
  for _ in $(seq 1 60); do
    grep -q "render_done" "$dir/log.txt" 2>/dev/null && break
    sleep 2
  done
  sleep 1
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  echo "== $tag ($fx @ $rate Hz)"
  grep -v '^param ' "$dir/log.txt" 2>/dev/null | sed 's/^/   /' || true
  # The plugin reports latency for the rate it is running at, which is the
  # audio device's rate (REAPER may not switch the device to the project rate).
  local run_rate; run_rate="$(sed -n 's/^device_srate=\([0-9]*\).*/\1/p' "$dir/log.txt" 2>/dev/null || true)"
  [ -n "$run_rate" ] || run_rate="$rate"
  local expected; expected="$("$R" latency "$run_rate")"
  if ! grep -q "render_done=true" "$dir/log.txt" 2>/dev/null; then echo "   FAIL: render did not complete"; fail=1; return; fi
  grep -q "^pdc=$expected\$" "$dir/log.txt" || { echo "   FAIL: expected PDC $expected"; fail=1; }
  "$R" check "$dir/out/reaper_render.wav" | sed 's/^/   /' || { echo "   FAIL: bad audio"; fail=1; }
}

run_case "VST3:ToneTinkerer" 48000 vst3_48k
run_case "CLAP:ToneTinkerer" 48000 clap_48k
run_case "VST3:ToneTinkerer" 44100 vst3_44k
run_case "CLAP:ToneTinkerer" 96000 clap_96k

echo "== REAPER vs offline renderer (48 kHz, whole render)"
lat="$("$R" latency 48000)"
for tag in vst3_48k clap_48k; do
  "$R" compare "$work/$tag/out/reaper_render.wav" "$work/offline_default.wav" "$lat" 10 | sed "s/^/   $tag: /" \
    || { echo "   FAIL: $tag differs from offline render"; fail=1; }
done
if [ "$fail" = 0 ]; then echo "REAPER verification PASSED"; else echo "REAPER verification FAILED"; exit 1; fi
