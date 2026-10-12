"""Echo time and ambience decay measured only in phrase-end tails.

After the guitar stops (a drop of >15 dB within 60 ms from a loud frame and
no new onset for 1.5 s), whatever remains is echo + reverb. The echo period
shows up as periodic bumps in the tail envelope; the decay slope gives RT60.
usage: python tails.py <wav> [label]
"""
import sys

import numpy as np
from scipy import signal

sys.path.insert(0, __file__.rsplit("/", 1)[0])
import analyze as a  # noqa: E402

x, sr = a.mono(sys.argv[1])
hop = int(0.005 * sr)
env = np.sqrt(np.convolve(x ** 2, np.ones(hop * 2) / (hop * 2), mode="same"))[::hop]
db = 20 * np.log10(env + 1e-9)
ref = np.percentile(db, 95)
tails = []
i = 20
while i < len(db) - 320:
    if db[i] > ref - 15 and db[i + 20] < db[i] - 10 and db[i + 20:i + 180].max() < db[i] - 6:
        tails.append(i + 12)
        i += 300
    else:
        i += 1
acs, slopes = [], []
for t in tails:
    seg = db[t:t + 180]  # 0.9 s
    tt = np.arange(len(seg)) * hop / sr
    good = seg > ref - 75
    if good.sum() > 60:
        k, b = np.polyfit(tt[good], seg[good], 1)
        if k < -2:
            slopes.append(k)
        resid = seg - (k * tt + b)
        resid -= resid.mean()
        ac = np.correlate(resid, resid, "full")[len(resid) - 1:]
        acs.append(ac / (ac[0] + 1e-9))
label = sys.argv[2] if len(sys.argv) > 2 else sys.argv[1]
if not acs:
    print(f"{label}: no clean phrase-end tails found")
    sys.exit(0)
ac = np.mean(acs, axis=0)
lags = np.arange(len(ac)) * hop / sr * 1000
sel = (lags >= 150) & (lags <= 800)
pk, pr = signal.find_peaks(ac[sel], prominence=0.02)
cands = sorted(zip(pr["prominences"], lags[sel][pk]), reverse=True)[:3]
print(f"{label}: {len(tails)} tails | RT60 ~{60 / -np.median(slopes):.1f} s (decay {np.median(slopes):.1f} dB/s) | echo period candidates: "
      + ", ".join(f"{int(l)} ms ({p:.2f})" for p, l in cands))
