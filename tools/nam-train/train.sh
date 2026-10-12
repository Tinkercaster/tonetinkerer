#!/usr/bin/env bash
# Train one bundled ToneTinkerer NAM model from the reference circuit.
#   tools/nam-train/train.sh <clean|crunch|lead> [epochs]
# Prerequisites: uv venv at tools/nam-train/.venv with `neural-amp-modeler`,
# and `cargo build --release -p tonetinkerer-render`.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../.."
voice="$1"; epochs="${2:-100}"
work="$here/work"; mkdir -p "$work"
render="$root/target/release/tonetinkerer-render"
[ -f "$work/di.wav" ] || "$render" gen-train "$work/di.wav" 240
"$render" refamp "$voice" "$work/di.wav" "$work/$voice.wav"
cat > "$work/data_$voice.json" <<JSON
{
  "train": {"start_seconds": null, "stop_seconds": -9.0, "ny": 8192},
  "validation": {"start_seconds": -9.0, "stop_seconds": null, "ny": null},
  "common": {"x_path": "$work/di.wav", "y_path": "$work/$voice.wav", "delay": 0}
}
JSON
python3 - "$here/configs/learning.json" "$work/learning_$voice.json" "$epochs" <<'PY'
import json, sys
c = json.load(open(sys.argv[1])); c["trainer"]["max_epochs"] = int(sys.argv[3])
json.dump(c, open(sys.argv[2], "w"), indent=2)
PY
rm -rf "$work/out_$voice"; mkdir -p "$work/out_$voice"
"$here/.venv/bin/nam-full" "$work/data_$voice.json" "$here/configs/model_wavenet_standard.json" \
  "$work/learning_$voice.json" "$work/out_$voice" --no-show
