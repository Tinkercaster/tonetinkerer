#!/usr/bin/env bash
# probe.sh <tag> <preset> "<overrides>" : render the pitch-matched DI and print tone features
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
W="$root/tools/nam-train/work/tonematch"
SKY_SET="$3" "$root/target/release/tonetinkerer-render" render "$2" "$W/$1_di.wav" "$W/probe.wav" 48000 256 2>/dev/null
"$root/tools/nam-train/.venv/bin/python" -c "
import sys; sys.path.insert(0,'$root/tools/tone-match'); import analyze as a
x,sr=a.mono('$W/probe.wav'); m,n=a.active_mask(x,sr); s=a.ltas(x,sr,m,n)
print('%-48s'%'$3'[:48], ' '.join('%5.1f'%v for v in s), '| dyn %.1f'%a.dynamics(x,sr,m,n))" 2>&1 | grep -v Warn
