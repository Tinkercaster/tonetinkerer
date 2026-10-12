"""Fit a preset's tone settings to a reference lead-guitar stem.

Renders the pitch-matched resynthesized DI through the real ToneTinkerer engine
(tonetinkerer-render, with SKY_SET overrides) and searches amp model, cabinet,
drive and EQ to minimise the 1/3-octave spectral difference to the reference.

usage: python fit.py <tag> <stem.wav> <preset> [fixed overrides]
Development-time only; reference material stays local.
"""
import itertools
import os
import subprocess
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import analyze as a  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
W = ROOT / "tools/nam-train/work/tonematch"
R = ROOT / "target/release/tonetinkerer-render"
tag, stem, preset = sys.argv[1], sys.argv[2], sys.argv[3]
fixed = sys.argv[4] if len(sys.argv) > 4 else ""
WIN = float(os.environ.get("FIT_WIN", "45"))
# Band weights: de-emphasise the extremes (MP3 / separation artefacts).
WEIGHTS = np.array([0.3, 0.7, 1, 1, 1, 1, 1, 1, 1, 1, 0.7, 0.3])
if os.environ.get("FIT_LOWS") == "light":
    # Stems with rhythm/acoustic bleed: trust the low end less.
    WEIGHTS = np.array([0.05, 0.1, 0.2, 0.6, 1, 1, 1, 1, 1, 1, 0.7, 0.3])

# 1. Fitting window.
if os.environ.get("FIT_DI"):
    # Given DI (e.g. a synthetic riff): pick the densest stretch of the stem by energy.
    xs, ssr = a.mono(stem)
    hopn = int(0.5 * ssr)
    fr = np.sqrt(np.mean(xs[: len(xs) // hopn * hopn].reshape(-1, hopn) ** 2, axis=1))
    k = int(WIN / 0.5)
    c = np.convolve((fr > np.percentile(fr, 40)).astype(float) * fr, np.ones(k), mode="valid")
    start = float(np.argmax(c)) * 0.5
else:
    # Densest stretch of lead playing from the pitch track.
    rows = np.loadtxt(W / f"{tag}.csv", delimiter=",")
    t, f0, rms = rows[:, 0], rows[:, 1], rows[:, 2]
    score = (f0 > 0) * (rms > np.percentile(rms, 50))
    hop = t[1] - t[0]
    k = int(WIN / hop)
    c = np.convolve(score, np.ones(k), mode="valid")
    start = t[int(np.argmax(c))]
print(f"{tag}: fitting window {start:.1f}-{start + WIN:.1f} s")

x, sr = a.mono(stem)
x = x[int(start * sr): int((start + WIN) * sr)]
m, n = a.active_mask(x, sr)
target = a.ltas(x, sr, m, n)

di_full = W / f"{tag}_di.wav"
di = W / f"{tag}_fitdi_{os.getpid()}.wav"
import soundfile as sf  # noqa: E402

d, dsr = sf.read(di_full)
if os.environ.get("FIT_DI"):
    sf.write(di, d[: int(WIN * dsr)], dsr, subtype="FLOAT")
else:
    sf.write(di, d[int(start * dsr): int((start + WIN) * dsr)], dsr, subtype="FLOAT")
out = W / f"{tag}_fit_{os.getpid()}.wav"

cache = {}


def evaluate(p):
    key = tuple(sorted(p.items()))
    if key in cache:
        return cache[key]
    sets = ",".join(f"{k}={v}" for k, v in p.items())
    if fixed and sets:
        sets = fixed + "," + sets
    elif fixed:
        sets = fixed
    env = dict(os.environ, SKY_SET=sets)
    subprocess.run([str(R), "render", preset, str(di), str(out), "48000", "512"], env=env, check=True, capture_output=True)
    y, ysr = a.mono(out)
    y = y[: int(WIN * ysr)]
    mm, nn = a.active_mask(y, ysr)
    sp = a.ltas(y, ysr, mm, nn)
    err = float(np.sqrt(np.sum(WEIGHTS * (sp - target) ** 2) / WEIGHTS.sum()))
    cache[key] = (err, sp)
    return cache[key]


CONT = {  # name: (min, max, initial step)
    "amp_gain_db": (-12.0, 6.0, 4.0),
    "mid": (-8.0, 12.0, 4.0),
    "mid_freq": (250.0, 1500.0, 300.0),
    "bass": (-6.0, 12.0, 3.0),
    "treble": (-8.0, 8.0, 3.0),
    "presence": (-8.0, 8.0, 3.0),
    "high_cut": (5000.0, 18000.0, 3000.0),
    "low_cut": (50.0, 140.0, 25.0),
    "drive_tone": (0.1, 0.9, 0.2),
    "body_eq": (-6.0, 9.0, 3.0),
    "drive": (0.05, 0.7, 0.15),
    "drive_level_db": (-6.0, 8.0, 3.0),
}
base = {"body_eq": 0.0, "drive": 0.3, "drive_level_db": 2.0, "amp_gain_db": -4.0, "mid": 4.0, "mid_freq": 500.0, "bass": 0.0, "treble": 0.0, "presence": 0.0, "high_cut": 11000.0, "low_cut": 80.0, "drive_tone": 0.5}

if os.environ.get("FIT_MODE") == "eval":
    # Score the sound as shipped (plus FIT_OVERRIDES, if given).
    fixed = os.environ.get("FIT_OVERRIDES", "")
    err, sp = evaluate({})
    print(f"SHIPPED {tag} ({preset}) rms band error {err:.2f} dB")
    print("  target ", " ".join(f"{v:5.1f}" for v in target))
    print("  shipped", " ".join(f"{v:5.1f}" for v in sp))
    sys.exit(0)

if os.environ.get("FIT_MODE") == "powell":
    from scipy.optimize import minimize

    disc = dict(kv.split("=") for kv in os.environ["FIT_DISCRETE"].split(","))
    disc = {k2: float(v) for k2, v in disc.items()}
    names = list(CONT)
    lo = np.array([CONT[k2][0] for k2 in names])
    hi = np.array([CONT[k2][1] for k2 in names])

    def f(u):
        q = dict(disc)
        q.update({k2: float(v) for k2, v in zip(names, lo + np.clip(u, 0, 1) * (hi - lo))})
        return evaluate(q)[0]

    start_p = dict(base, **{k2: v for k2, v in disc.items() if k2 in base})
    u0 = np.array([(start_p[k2] - CONT[k2][0]) / (CONT[k2][1] - CONT[k2][0]) for k2 in names])
    res = minimize(f, u0, method="Powell", bounds=[(0, 1)] * len(names), options={"maxfev": int(os.environ.get("FIT_EVALS", "140")), "xtol": 0.02, "ftol": 0.01})
    best = dict(disc)
    best.update({k2: float(v) for k2, v in zip(names, lo + np.clip(res.x, 0, 1) * (hi - lo))})
    err, sp = evaluate(best)
    print(f"RESULT {tag} [{os.environ['FIT_DISCRETE']}] rms band error {err:.2f} dB ({len(cache)} renders)")
    print("  target ", " ".join(f"{v:5.1f}" for v in target))
    print("  fitted ", " ".join(f"{v:5.1f}" for v in sp))
    print("  params ", {k2: round(v, 2) for k2, v in best.items()})
    sys.exit(0)

# 2. Discrete search (amp model x cab x drive mode) at base settings.
results = []
for amp, cab, drive in itertools.product([1, 2], [0, 1, 2], ["drive_on=0", "drive_on=1;drive_fat=1", "drive_on=1;drive_fat=0"]):
    p = dict(base, amp_model=amp, cab_ir=cab)
    for kv in drive.split(";"):
        k2, v = kv.split("=")
        p[k2] = float(v)
    err, _ = evaluate(p)
    results.append((err, p))
results.sort(key=lambda r: r[0])
print("best discrete:", [(round(e, 2), {k: p[k] for k in ("amp_model", "cab_ir", "drive_on")} | ({"fat": p.get("drive_fat")})) for e, p in results[:3]])

# 3. Coordinate descent on the two best discrete candidates.
best_err, best_p = 1e9, None
for err, p in results[:2]:
    p = dict(p)
    steps = {k: v[2] for k, v in CONT.items()}
    for _ in range(4):
        improved = False
        for name, (lo, hi, _) in CONT.items():
            for sgn in (+1, -1):
                q = dict(p)
                q[name] = float(np.clip(p[name] + sgn * steps[name], lo, hi))
                e2, _ = evaluate(q)
                if e2 < err - 1e-3:
                    err, p, improved = e2, q, True
                    break
        for k2 in steps:
            steps[k2] *= 0.5
        if not improved and all(steps[k2] < CONT[k2][2] / 8 for k2 in steps):
            break
    if err < best_err:
        best_err, best_p = err, p

err, sp = evaluate(best_p)
print(f"RESULT {tag} rms band error {err:.2f} dB")
print("  target ", " ".join(f"{v:5.1f}" for v in target))
print("  fitted ", " ".join(f"{v:5.1f}" for v in sp))
print("  params ", {k: (round(v, 1) if isinstance(v, float) else v) for k, v in best_p.items()})
