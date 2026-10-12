"""Pitch-track a (separated) lead-guitar stem for pitch-matched resynthesis.

usage: python f0.py <guitar.wav> <out.csv> [start_s] [dur_s]
CSV columns: time_s, f0_hz (0 = unvoiced), rms_db, onset (0/1)
"""
import sys

import librosa
import numpy as np

path, out = sys.argv[1], sys.argv[2]
start = float(sys.argv[3]) if len(sys.argv) > 3 else 0.0
dur = float(sys.argv[4]) if len(sys.argv) > 4 else None
sr = 22050
hop = 256
y, _ = librosa.load(path, sr=sr, mono=True, offset=start, duration=dur)
f0, voiced, prob = librosa.pyin(y, fmin=80.0, fmax=1600.0, sr=sr, frame_length=2048, hop_length=hop)
rms = librosa.feature.rms(y=y, frame_length=2048, hop_length=hop)[0]
on = np.zeros(len(f0), dtype=int)
on_frames = librosa.onset.onset_detect(y=y, sr=sr, hop_length=hop, backtrack=False)
on[on_frames[on_frames < len(on)]] = 1
n = min(len(f0), len(rms))
with open(out, "w") as fh:
    for i in range(n):
        f = f0[i] if (voiced[i] and np.isfinite(f0[i])) else 0.0
        fh.write(f"{i * hop / sr:.5f},{f:.3f},{20 * np.log10(rms[i] + 1e-9):.2f},{on[i]}\n")
v = np.array([x for x in f0[:n] if np.isfinite(x)])
print(f"{out}: {n} frames, voiced {np.mean(voiced[:n]) * 100:.0f}%, median f0 {np.median(v):.0f} Hz, range {np.percentile(v, 5):.0f}-{np.percentile(v, 95):.0f} Hz")
