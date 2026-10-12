"""Measure lead-guitar tone features from separated guitar stems and compare
them with ToneTinkerer preset renders.

Development-time tool only: reference recordings and anything derived from
them stay local (git-ignored); nothing here ships in the plugin.

usage: python analyze.py <stems_root> <renders_dir> [song=preset ...]
  stems_root  : demucs output dir containing <song>/guitar.wav
  renders_dir : tonetinkerer-render outputs (stereo wav per preset)
  song=preset : pairs to compare, e.g. "Cryin'=07_crying_voice"
"""
import sys
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy import signal

BANDS = [80, 125, 200, 315, 500, 800, 1250, 2000, 3150, 5000, 8000, 12500]


def mono(path):
    x, sr = sf.read(path, always_2d=True)
    return x.mean(axis=1), sr


def frames_rms(x, sr, win=0.05):
    n = int(win * sr)
    m = len(x) // n
    f = x[: m * n].reshape(m, n)
    return np.sqrt((f ** 2).mean(axis=1) + 1e-20), n


def active_mask(x, sr, rel_db=-24.0):
    """Frames where the guitar is clearly playing (lead passages)."""
    r, n = frames_rms(x, sr)
    db = 20 * np.log10(r)
    ref = np.percentile(db, 95)
    return db > ref + rel_db, n


def ltas(x, sr, mask=None, n=None):
    if mask is not None:
        segs = [x[i * n:(i + 1) * n] for i in np.flatnonzero(mask)]
        x = np.concatenate(segs) if segs else x
    f, p = signal.welch(x, sr, nperseg=8192)
    db = 10 * np.log10(p + 1e-30)
    # 1/3-octave smoothing at the band centres, normalised to 1 kHz.
    out = []
    for fc in BANDS:
        lo, hi = fc / 2 ** (1 / 6), fc * 2 ** (1 / 6)
        sel = (f >= lo) & (f < hi)
        out.append(10 * np.log10(np.mean(10 ** (db[sel] / 10))))
    out = np.array(out)
    k = np.interp(np.log(1000), np.log(BANDS), out)
    return out - k


def rolloff(x, sr, mask, n, drop_db=12.0):
    """Frequency above the spectral peak region where level falls drop_db below the 1-3 kHz plateau."""
    segs = [x[i * n:(i + 1) * n] for i in np.flatnonzero(mask)]
    x = np.concatenate(segs)
    f, p = signal.welch(x, sr, nperseg=8192)
    db = 10 * np.log10(p + 1e-30)
    plateau = db[(f > 1000) & (f < 3000)].mean()
    above = np.flatnonzero((f > 3000) & (db < plateau - drop_db))
    return f[above[0]] if len(above) else float("nan")


def echo_estimate(x, sr):
    """Echo delay from the autocorrelation of the onset-strength envelope."""
    hop = int(0.005 * sr)
    env = np.sqrt(np.convolve(x ** 2, np.ones(hop) / hop, mode="same"))[::hop]
    on = np.maximum(np.diff(np.log(env + 1e-6)), 0)
    on -= on.mean()
    ac = np.correlate(on, on, mode="full")[len(on) - 1:]
    ac /= ac[0]
    lags = np.arange(len(ac)) * hop / sr
    sel = (lags > 0.18) & (lags < 1.0)
    idx = np.flatnonzero(sel)
    peaks, props = signal.find_peaks(ac[idx], prominence=0.01)
    if len(peaks) == 0:
        return None
    best = sorted(peaks, key=lambda i: -props["prominences"][list(peaks).index(i)])[:3]
    return [(round(lags[idx[i]] * 1000), round(float(ac[idx[i]]), 3)) for i in best]


def decay_estimate(x, sr):
    """Median decay rate after phrase ends (dB/s) -> RT60-ish seconds."""
    r, n = frames_rms(x, sr, 0.02)
    db = 20 * np.log10(r)
    ref = np.percentile(db, 95)
    rates = []
    i = 5
    while i < len(db) - 40:
        # A phrase end: drop of > 12 dB within 100 ms from a loud frame.
        if db[i] > ref - 15 and db[i + 5] < db[i] - 12:
            tail = db[i + 5:i + 40]
            t = np.arange(len(tail)) * 0.02
            good = tail > ref - 70
            if good.sum() > 8:
                slope = np.polyfit(t[good], tail[good], 1)[0]
                if slope < -3:
                    rates.append(slope)
            i += 40
        else:
            i += 1
    if not rates:
        return None
    return 60.0 / -np.median(rates), len(rates)


def dynamics(x, sr, mask, n):
    r, _ = frames_rms(x, sr)
    db = 20 * np.log10(r[mask])
    return float(np.percentile(db, 90) - np.percentile(db, 10))


def describe(name, x, sr):
    mask, n = active_mask(x, sr)
    sp = ltas(x, sr, mask, n)
    return {
        "name": name,
        "ltas": sp,
        "rolloff12": rolloff(x, sr, mask, n),
        "echo": echo_estimate(x * 1.0, sr),
        "decay": decay_estimate(x, sr),
        "dyn": dynamics(x, sr, mask, n),
        "mid_peak": BANDS[int(np.argmax(sp[3:9])) + 3],
    }


def show(d):
    bands = " ".join(f"{b if b < 1000 else str(b // 1000) + 'k':>5}" for b in BANDS)
    vals = " ".join(f"{v:5.1f}" for v in d["ltas"])
    print(f"  {d['name']}")
    print(f"    LTAS (dB re 1 kHz) {bands}")
    print(f"                       {vals}")
    print(f"    mid peak ~{d['mid_peak']} Hz | -12 dB roll-off ~{d['rolloff12']:.0f} Hz | dynamics (p90-p10) {d['dyn']:.1f} dB")
    print(f"    echo candidates (ms, strength): {d['echo']} | tail decay RT60~ {d['decay']}")


def compare(ref_path, ours_path, label):
    x, sr = mono(ref_path)
    y, sr2 = mono(ours_path)
    n = min(len(x), int(len(y) * sr / sr2))
    ref = describe(f"REFERENCE {label}", x[:n], sr)
    ours = describe(f"TONETINKERER {Path(ours_path).stem}", y, sr2)
    print(f"=== {label}")
    show(ref)
    show(ours)
    diff = ref["ltas"] - ours["ltas"]
    print("    EQ needed (ref - ours):  " + " ".join(f"{v:5.1f}" for v in diff))
    print()
    return diff


if __name__ == "__main__" and sys.argv[1] == "pair":
    compare(sys.argv[2], sys.argv[3], sys.argv[4])
    sys.exit(0)

if __name__ == "__main__":
    stems = Path(sys.argv[1])
    renders = Path(sys.argv[2])
    pairs = [a.split("=", 1) for a in sys.argv[3:]]
    for song, preset in pairs:
        g = next(stems.glob(f"*{song}*/guitar.wav"))
        x, sr = mono(g)
        ref = describe(f"REFERENCE {song} (guitar stem)", x, sr)
        y, sr2 = mono(next(renders.glob(f"{preset}*.wav")))
        ours = describe(f"TONETINKERER {preset}", y, sr2)
        print(f"=== {song}  vs  {preset}")
        show(ref)
        show(ours)
        diff = ref["ltas"] - ours["ltas"]
        print("    EQ needed (ref - ours):  " + " ".join(f"{v:5.1f}" for v in diff))
        print()
