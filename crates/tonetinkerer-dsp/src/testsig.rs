//! Synthetic guitar DI signals (development and tests only; not used by the
//! plugin): the NAM training signal, the demo phrase used for loudness
//! matching and listening, and pitch-matched resynthesis of reference lines.
//!
//! Originally the NAM training signal generator.
//!
//! A physically-inspired plucked-string synthesizer (Karplus-Strong with a
//! fractional-delay tuning allpass, string damping, pick-position comb and
//! pickup roll-off) generates single notes, legato phrases with bends and
//! vibrato, and chords across the dynamic range a guitar through an audio
//! interface's instrument input produces (roughly -45 to 0 dBFS peaks).
//! Sweeps and noise bursts fill in the spectrum. Deterministic (seeded).

use crate::biquad::{Biquad, Coeffs};
use std::f32::consts::PI;

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        self.next_u32() as f32 / 4_294_967_296.0
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }
}

struct KsString {
    buf: Vec<f32>,
    pos: usize,
    lp: f32,
    ap_x1: f32,
    ap_y1: f32,
    damping: f32,
}

impl KsString {
    fn new(sr: f32) -> Self {
        Self {
            buf: vec![0.0; (sr / 40.0) as usize + 4],
            pos: 0,
            lp: 0.0,
            ap_x1: 0.0,
            ap_y1: 0.0,
            damping: 0.5,
        }
    }

    fn pluck(&mut self, rng: &mut Rng, amp: f32, brightness: f32, period: f32) {
        let n = (period.floor() as usize).clamp(2, self.buf.len() - 1);
        self.buf.fill(0.0);
        // Low-passed noise burst excitation; brightness = pick attack.
        let mut s = 0.0;
        for i in 0..n {
            let w = rng.range(-1.0, 1.0);
            s += (w - s) * (0.2 + 0.8 * brightness);
            self.buf[i] = s * amp;
        }
        // The read head trails the write head by `period`, so starting the
        // write head at n makes the first reads hit the excitation.
        self.pos = n;
        self.lp = 0.0;
        self.damping = 0.35 + 0.5 * brightness;
    }

    /// Advance one sample with a (possibly time-varying) period in samples.
    fn tick(&mut self, period: f32, decay: f32) -> f32 {
        let len = self.buf.len();
        let int = (period.floor() as usize).clamp(2, len - 2);
        let frac = period - int as f32;
        let read = (self.pos + len - int) % len;
        let x = self.buf[read];
        // Tuning allpass for the fractional part.
        let c = (1.0 - frac) / (1.0 + frac);
        let ap = c * x + self.ap_x1 - c * self.ap_y1;
        self.ap_x1 = x;
        self.ap_y1 = ap;
        // Loss filter.
        self.lp += (ap - self.lp) * self.damping;
        let y = self.lp * decay;
        self.buf[self.pos] = y;
        self.pos = (self.pos + 1) % len;
        y
    }
}

fn midi_to_hz(n: f32) -> f32 {
    440.0 * 2f32.powf((n - 69.0) / 12.0)
}

/// Pickup / cable roll-off and a little pick-position comb.
struct Pickup {
    lp1: f32,
    lp2: f32,
    coeff: f32,
}

impl Pickup {
    fn new(sr: f32, cutoff: f32) -> Self {
        Self {
            lp1: 0.0,
            lp2: 0.0,
            coeff: 1.0 - (-2.0 * PI * cutoff / sr).exp(),
        }
    }
    fn process(&mut self, x: f32) -> f32 {
        self.lp1 += (x - self.lp1) * self.coeff;
        self.lp2 += (self.lp1 - self.lp2) * self.coeff;
        self.lp2
    }
}

pub fn add_note(
    out: &mut Vec<f32>,
    sr: f32,
    rng: &mut Rng,
    start: usize,
    pitches: &[(f32, f32)],
    dur: f32,
    amp: f32,
    vibrato: f32,
) {
    // `pitches`: list of (time_fraction, midi_note); pitch glides between them (bends / slides).
    let n = (dur * sr) as usize;
    if out.len() < start + n {
        out.resize(start + n, 0.0);
    }
    let mut s = KsString::new(sr);
    let p0 = sr / midi_to_hz(pitches[0].1);
    let bright = rng.range(0.3, 1.0);
    s.pluck(rng, amp, bright, p0);
    let mut pu = Pickup::new(sr, rng.range(3_500.0, 6_500.0));
    let vib_rate = rng.range(4.5, 6.5);
    let decay = 0.9985 + 0.0013 * rng.f();
    for i in 0..n {
        let t = i as f32 / n as f32;
        // Piecewise-linear glide through the pitch points.
        let mut note = pitches[0].1;
        for w in pitches.windows(2) {
            if t >= w[0].0 && t <= w[1].0 {
                let k = ((t - w[0].0) / (w[1].0 - w[0].0).max(1e-6)).clamp(0.0, 1.0);
                let k = k * k * (3.0 - 2.0 * k);
                note = w[0].1 + (w[1].1 - w[0].1) * k;
            } else if t > w[1].0 {
                note = w[1].1;
            }
        }
        let vib = vibrato * (2.0 * PI * vib_rate * i as f32 / sr).sin() * t.min(0.5) * 2.0;
        let period = sr / midi_to_hz(note + vib);
        let y = s.tick(period, decay);
        // Fade the very end to avoid clicks.
        let tail = ((n - i) as f32 / (0.01 * sr)).min(1.0);
        out[start + i] += pu.process(y) * tail;
    }
}

pub fn generate(sr: f32, seconds: f32, seed: u64) -> Vec<f32> {
    let total = (seconds * sr) as usize;
    let mut rng = Rng::new(seed);
    let mut out = vec![0.0; total];
    let mut t = (0.25 * sr) as usize;

    // Dynamic levels in dBFS peak, cycling so every section spans the range.
    let levels = [-42.0, -30.0, -22.0, -16.0, -12.0, -8.0, -5.0, -2.0, 0.0];
    let mut li = 0;
    let mut next_level = |rng: &mut Rng| {
        li = (li + 1 + (rng.next_u32() % 3) as usize) % levels.len();
        10f32.powf((levels[li] + rng.range(-2.0, 2.0)) / 20.0)
    };

    // Section 1 (about 55%): single notes and legato phrases with bends and vibrato.
    while t < total * 55 / 100 {
        let amp = next_level(&mut rng);
        let base = 40.0 + rng.range(0.0, 46.0).floor();
        let dur = rng.range(0.25, 2.8);
        let mode = rng.next_u32() % 4;
        let pitches: Vec<(f32, f32)> = match mode {
            0 => vec![(0.0, base)],
            1 => vec![(0.0, base), (0.15, base), (0.35, base + 2.0)], // whole-step bend
            2 => vec![(0.0, base), (0.4, base + rng.range(-5.0, 5.0).round())], // slide
            _ => vec![
                (0.0, base),
                (0.1, base + 1.0),
                (0.2, base),
                (0.5, base + 3.0),
            ],
        };
        let vib = if rng.f() < 0.6 {
            rng.range(0.1, 0.45)
        } else {
            0.0
        };
        add_note(&mut out, sr, &mut rng, t, &pitches, dur, amp * 2.5, vib);
        // Mostly legato overlaps, sometimes gaps for decays into silence.
        t += (dur * sr * rng.range(0.35, 1.1)) as usize;
    }

    // Section 2 (about 25%): chords, strums and palm-muted chugs.
    while t < total * 80 / 100 {
        let amp = next_level(&mut rng);
        let root = 40.0 + rng.range(0.0, 12.0).floor();
        let shape: &[f32] = match rng.next_u32() % 3 {
            0 => &[0.0, 7.0, 12.0],                   // power chord
            1 => &[0.0, 7.0, 12.0, 16.0, 19.0, 24.0], // open major
            _ => &[0.0, 7.0, 10.0, 15.0, 19.0],       // minor 7
        };
        let muted = rng.f() < 0.35;
        let dur = if muted {
            rng.range(0.12, 0.3)
        } else {
            rng.range(0.6, 2.5)
        };
        let strum = rng.range(0.004, 0.025);
        for (k, iv) in shape.iter().enumerate() {
            let st = t + (k as f32 * strum * sr) as usize;
            add_note(
                &mut out,
                sr,
                &mut rng,
                st,
                &[(0.0, root + iv)],
                dur,
                amp * 1.4,
                0.0,
            );
        }
        t += ((dur + rng.range(0.0, 0.25)) * sr) as usize;
    }

    // Section 3: exponential sine sweeps at several levels.
    for &lvl in &[0.02f32, 0.12, 0.45, 0.9] {
        let d = 2.5;
        let n = (d * sr) as usize;
        let (f0, f1) = (40.0f32, 9_000.0f32);
        let k = (f1 / f0).ln();
        for i in 0..n {
            if t + i >= total {
                break;
            }
            let x = i as f32 / n as f32;
            let phase = 2.0 * PI * f0 * d * ((k * x).exp() - 1.0) / k;
            let env = (x * 40.0).min(1.0).min((1.0 - x) * 40.0);
            out[t + i] += lvl * env * phase.sin();
        }
        t += n + (0.2 * sr) as usize;
    }

    // Section 4: band-limited noise bursts at several levels, then more
    // notes until the end (the last 9 s become NAM's validation split).
    let mut lp = 0.0;
    for &lvl in &[0.01f32, 0.08, 0.3, 0.7] {
        let n = (0.8 * sr) as usize;
        for i in 0..n {
            if t + i >= total {
                break;
            }
            lp += (rng.range(-1.0, 1.0) - lp) * 0.3;
            out[t + i] += lvl * lp * 1.7;
        }
        t += n + (0.15 * sr) as usize;
    }
    // NAM requires >= 0.4 s of silence before the validation split (last 9 s)
    // so no response to earlier input leaks into it.
    let val_start = total - (9.0 * sr) as usize;
    let gap_start = val_start - (0.6 * sr) as usize;
    t = t.max(val_start);
    while t < total.saturating_sub((0.5 * sr) as usize) {
        let amp = next_level(&mut rng);
        let base = 40.0 + rng.range(0.0, 40.0).floor();
        let dur = rng.range(0.3, 1.8).min((total - t) as f32 / sr - 0.05);
        if dur < 0.1 {
            break;
        }
        add_note(
            &mut out,
            sr,
            &mut rng,
            t,
            &[(0.0, base), (0.3, base + 2.0)],
            dur,
            amp * 2.5,
            0.25,
        );
        t += (dur * sr * 0.8) as usize;
    }

    out.truncate(total);
    let fade = (0.05 * sr) as usize;
    for i in 0..fade {
        out[gap_start - fade + i] *= 1.0 - i as f32 / fade as f32;
    }
    out[gap_start..val_start].fill(0.0);
    // Keep within float headroom while retaining the hot end of the range.
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 1.0 {
        for v in &mut out {
            *v /= peak;
        }
    }
    out
}

/// How hot a typical guitar arrives from an interface, relative to
/// `demo_phrase` (which peaks around -6 dBFS, hotter than most real setups):
/// peaks around -18 dBFS. Sounds are level-matched at this level, so a clean
/// sound (whose loudness follows the input 1:1) sits with the distorted ones
/// for typical players, without boosting the input (and its hiss) before the
/// amps.
pub const VOICING_LEVEL_DB: f32 = -12.0;

/// `demo_phrase` at the typical playing level (`VOICING_LEVEL_DB`): the
/// reference the sounds are level-matched against.
pub fn voicing_phrase(sr: f32) -> Vec<f32> {
    let g = 10f32.powf(VOICING_LEVEL_DB / 20.0);
    demo_phrase(sr).into_iter().map(|v| v * g).collect()
}

/// A short legato lead phrase (bends, vibrato, slides, a held note) plus a
/// chord, for demo renders and integration tests. Peaks around -6 dBFS.
pub fn demo_phrase(sr: f32) -> Vec<f32> {
    let mut rng = Rng::new(7);
    let mut out = vec![0.0; (sr * 9.0) as usize];
    let notes: &[(f32, &[(f32, f32)], f32, f32)] = &[
        (0.20, &[(0.0, 64.0)], 0.30, 0.0),
        (0.45, &[(0.0, 66.0)], 0.30, 0.0),
        (0.70, &[(0.0, 67.0), (0.25, 67.0), (0.45, 69.0)], 1.10, 0.3),
        (1.75, &[(0.0, 71.0)], 0.25, 0.0),
        (1.95, &[(0.0, 69.0)], 0.25, 0.0),
        (2.15, &[(0.0, 67.0), (0.5, 64.0)], 0.70, 0.2),
        (2.90, &[(0.0, 76.0), (0.15, 76.0), (0.35, 78.0)], 2.40, 0.4),
        (5.40, &[(0.0, 52.0)], 2.50, 0.0),
        (5.40, &[(0.0, 59.0)], 2.50, 0.0),
        (5.41, &[(0.0, 64.0)], 2.50, 0.0),
    ];
    for &(t, pitches, dur, vib) in notes {
        add_note(
            &mut out,
            sr,
            &mut rng,
            (t * sr) as usize,
            pitches,
            dur,
            0.9,
            vib,
        );
    }
    out.truncate((sr * 9.0) as usize);
    realism(&mut out, sr);
    out
}

/// One analysis frame of a reference lead line.
pub struct F0Frame {
    pub t: f32,
    pub f0: f32,
    pub rms_db: f32,
    pub onset: bool,
}

/// Resynthesize a DI that plays the same notes as a pitch-tracked reference:
/// same pitches, timing, bends and vibrato (the string follows the f0 contour
/// continuously), re-plucked at detected onsets and big pitch jumps, pick
/// strength following the reference dynamics. Gives a like-for-like source for
/// comparing amp/cab/EQ voicing against a recording.
pub fn resynth(frames: &[F0Frame], sr: f32) -> Vec<f32> {
    let mut rng = Rng::new(11);
    let end = frames.last().map_or(0.0, |f| f.t) + 1.0;
    let n = (end * sr) as usize;
    let mut out = vec![0.0f32; n];
    let mut s = KsString::new(sr);
    let mut pu = Pickup::new(sr, 5_000.0);
    let peak_db = frames.iter().map(|f| f.rms_db).fold(-200.0f32, f32::max);
    let mut sounding = false;
    let mut last_f0 = 0.0f32;
    let mut mute = 1.0f32;
    for k in 0..frames.len().saturating_sub(1) {
        let (a, b) = (&frames[k], &frames[k + 1]);
        let i0 = (a.t * sr) as usize;
        let i1 = ((b.t * sr) as usize).min(n);
        let voiced = a.f0 > 0.0;
        if voiced {
            let jump = last_f0 > 0.0 && (12.0 * (a.f0 / last_f0).log2()).abs() > 1.5;
            if !sounding || a.onset || jump {
                // Pick strength from the reference level (DI-like range -40..-6 dBFS).
                let rel = (a.rms_db - peak_db).clamp(-30.0, 0.0);
                let amp = 10f32.powf((-8.0 + rel * 0.8) / 20.0) * 2.0;
                let bright = rng.range(0.55, 0.9);
                s.pluck(&mut rng, amp, bright, sr / a.f0);
                sounding = true;
            }
            last_f0 = a.f0;
            mute = 1.0;
        } else if sounding {
            sounding = false;
        }
        let f_a = if a.f0 > 0.0 { a.f0 } else { last_f0.max(80.0) };
        let f_b = if b.f0 > 0.0 { b.f0 } else { f_a };
        for i in i0..i1 {
            let t = (i - i0) as f32 / (i1 - i0).max(1) as f32;
            let f = f_a * (f_b / f_a).powf(t);
            if !voiced {
                // Fret-hand damping between phrases.
                mute *= 0.9992;
            }
            let y = s.tick(sr / f.max(60.0), 0.9996);
            out[i] += pu.process(y) * mute;
        }
    }
    realism(&mut out, sr);
    out
}

/// A metal rhythm riff DI: palm-muted sixteenth-note chugs on a power chord
/// (root + fifth + octave) with ringing accents and occasional higher fills.
/// `root_midi` sets the tuning (40 = E standard, 38 = drop D, 36 = drop C).
pub fn metal_riff(sr: f32, root_midi: f32, bpm: f32, seconds: f32) -> Vec<f32> {
    let mut rng = Rng::new(23);
    let total = (seconds * sr) as usize;
    let mut out = vec![0.0f32; total];
    let six = 60.0 / bpm / 4.0; // a sixteenth note in seconds
    let chord = [0.0f32, 7.0, 12.0];
    let mut t = 0.1f32;
    let mut step = 0usize;
    while t < seconds - 1.0 {
        let bar_pos = step % 16;
        let start = (t * sr) as usize;
        if bar_pos == 0 || bar_pos == 6 || (bar_pos == 12 && step % 32 >= 16) {
            // Accent: open, ringing power chord.
            let dur = six * if bar_pos == 12 { 4.0 } else { 2.0 } * 0.95;
            let shift = if step % 64 >= 32 && bar_pos == 6 {
                3.0
            } else {
                0.0
            };
            for (k, iv) in chord.iter().enumerate() {
                let st = start + (k as f32 * 0.004 * sr) as usize;
                add_note(
                    &mut out,
                    sr,
                    &mut rng,
                    st,
                    &[(0.0, root_midi + shift + iv)],
                    dur,
                    0.7,
                    0.0,
                );
            }
            t += six * if bar_pos == 12 { 4.0 } else { 2.0 };
            step += if bar_pos == 12 { 4 } else { 2 };
        } else if step % 32 == 14 {
            // Fill: two quick higher notes (minor pentatonic).
            add_note(
                &mut out,
                sr,
                &mut rng,
                start,
                &[(0.0, root_midi + 15.0)],
                six * 0.9,
                0.5,
                0.0,
            );
            add_note(
                &mut out,
                sr,
                &mut rng,
                start + (six * sr) as usize,
                &[(0.0, root_midi + 17.0)],
                six * 0.9,
                0.5,
                0.0,
            );
            t += 2.0 * six;
            step += 2;
        } else {
            // Palm-muted chug: root + fifth, very short.
            for iv in [0.0f32, 7.0] {
                add_note(
                    &mut out,
                    sr,
                    &mut rng,
                    start,
                    &[(0.0, root_midi + iv)],
                    six * 0.55,
                    0.8,
                    0.0,
                );
            }
            t += six;
            step += 1;
        }
    }
    out.truncate(total);
    realism(&mut out, sr);
    out
}

/// Make a synthetic DI spectrally realistic. The plucked-string model is far
/// brighter than real strings + magnetic pickups (measured against a real
/// guitar DI: about 10-25 dB too much energy above 3 kHz and too little low
/// end), which would bias any voicing done with it. This applies the
/// pickup-resonance roll-off and a gentle low-end lift measured from that
/// comparison (see IMPLEMENTATION_PLAN.md, M8).
pub fn realism(x: &mut [f32], sr: f32) {
    // Fitted (least squares, 1-12.5 kHz) to the real DI's roll-off relative
    // to the synthetic training DI; low shelf from the low-band comparison.
    let mut f = [
        Biquad::new(Coeffs::lowpass(sr, 1_800.0, 0.75)),
        Biquad::new(Coeffs::peaking(sr, 7_850.0, 0.91, -7.0)),
        Biquad::new(Coeffs::high_shelf(sr, 8_870.0, 0.7, 10.2)),
        Biquad::new(Coeffs::low_shelf(sr, 600.0, 0.7, 6.0)),
    ];
    for v in x.iter_mut() {
        let mut y = *v;
        for b in f.iter_mut() {
            y = b.process(y);
        }
        *v = y;
    }
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        x.iter_mut().for_each(|v| *v *= 0.5 / peak);
    }
}
