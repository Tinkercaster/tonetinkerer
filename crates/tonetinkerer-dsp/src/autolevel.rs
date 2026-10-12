//! Automatic input level: brings any guitar / interface to the level the
//! sounds are voiced and level-matched at.
//!
//! Why: a clean amp's loudness follows the input level 1:1, a high-gain amp's
//! barely moves. Sounds matched at one input level drift apart at any other:
//! with a quiet interface clean sounds end up 6-12 dB below the distorted
//! ones. So the input is normalised: during the first seconds of playing the
//! stage measures how loud the playing is and sets a gain that puts it where
//! the sounds were voiced. Then it holds that gain, so the player's own
//! dynamics are kept, and only learns again if the input level changes a lot
//! for a long time (another guitar, the interface gain turned) or when asked.
//!
//! How loud the playing is: the 90th percentile of the frame peaks heard
//! while playing (a histogram, 1 dB bins). The loud end of the playing,
//! robust against a light touch of the strings, quiet notes, decays and
//! single stray peaks (a running average seeded by the first "attack" once
//! learned a light touch as the playing level and boosted everything ~20 dB).

use crate::util::{Smoothed, db_to_gain};

/// 90th-percentile frame peak of the voicing / level-matching input (the
/// demo phrase, `testsig::demo_phrase`), measured with this stage's own
/// estimator: playing that loud needs no correction.
pub const REFERENCE_LEVEL_DB: f32 = -12.0;
/// Gain range the stage may apply.
const MIN_DB: f32 = -12.0;
const MAX_DB: f32 = 30.0;
/// Seconds of playing used to learn the level.
const LEARN_S: f32 = 6.0;
/// Relearn when the playing level stays this far off for this long.
const DRIFT_DB: f32 = 10.0;
const DRIFT_S: f32 = 15.0;
/// Measurement frame: decisions happen every FRAME samples of input,
/// whatever the host's block sizes, so output never depends on them.
const FRAME: usize = 64;
/// Starting hiss estimate (mean square, -40 dBFS): deliberately high, it
/// falls to the real hiss at the first quiet frames.
const FLOOR_START: f32 = 1e-4;
/// Only frames this far above the input's own hiss count as playing.
const PLAYING_ABOVE_HISS_DB: f32 = 20.0;
/// Listen this long before learning anything (the hiss estimate settles).
const SETTLE_S: f32 = 0.3;
/// Histogram of playing frame peaks: 1 dB bins from -100 to 0 dBFS.
const BINS: usize = 100;
/// The percentile that stands for "how loud the playing is".
const PERCENTILE: f32 = 0.9;

pub struct AutoLevel {
    sr: f32,
    gain: Smoothed,
    gain_db: f32,
    /// Playing frame peaks since (re)learning started, and a slowly
    /// forgetting copy for drift detection.
    learn_hist: [f32; BINS],
    recent_hist: [f32; BINS],
    learned_s: f32,
    drift_s: f32,
    frame_pos: usize,
    frame_peak: f32,
    frame_sq: f32,
    /// The input's own hiss level (frame mean square), learned in quiet
    /// moments, and how long the input has been observed.
    floor: f32,
    observed_s: f32,
}

impl AutoLevel {
    pub fn new(sample_rate: f32) -> Self {
        let mut gain = Smoothed::new(1.0);
        // Slow enough that a level change while learning is a gentle swell.
        gain.set_time(sample_rate, 400.0);
        Self {
            sr: sample_rate,
            gain,
            gain_db: 0.0,
            learn_hist: [0.0; BINS],
            recent_hist: [0.0; BINS],
            learned_s: 0.0,
            drift_s: 0.0,
            frame_pos: 0,
            frame_peak: 0.0,
            frame_sq: 0.0,
            floor: FLOOR_START,
            observed_s: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.gain.reset(1.0);
        self.gain_db = 0.0;
        self.learn_hist = [0.0; BINS];
        self.recent_hist = [0.0; BINS];
        self.learned_s = 0.0;
        self.drift_s = 0.0;
        self.frame_pos = 0;
        self.frame_peak = 0.0;
        self.frame_sq = 0.0;
        self.floor = FLOOR_START;
        self.observed_s = 0.0;
    }

    /// Learn again from the next seconds of playing (the current gain stays
    /// until the new level is known).
    pub fn relearn(&mut self) {
        self.learned_s = 0.0;
        self.drift_s = 0.0;
        self.learn_hist = [0.0; BINS];
    }

    /// The gain currently aimed at (dB).
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// Whether the level has been learned (playing heard for long enough).
    pub fn learned(&self) -> bool {
        self.learned_s >= LEARN_S
    }

    /// Measure `buf` (the raw input) and apply the gain.
    pub fn process(&mut self, buf: &mut [f32]) {
        for x in buf.iter_mut() {
            self.frame_peak = self.frame_peak.max(x.abs());
            self.frame_sq += *x * *x;
            self.frame_pos += 1;
            if self.frame_pos == FRAME {
                let (peak, ms) = (self.frame_peak, self.frame_sq / FRAME as f32);
                self.frame_pos = 0;
                self.frame_peak = 0.0;
                self.frame_sq = 0.0;
                self.update(peak, ms);
            }
            *x *= self.gain.next();
        }
    }

    /// One measurement frame.
    fn update(&mut self, frame_peak: f32, frame_ms: f32) {
        let secs = FRAME as f32 / self.sr;
        // The input's hiss: the quietest recent frames (rises slowly, so a
        // long note can't be taken for hiss within the learning time).
        if frame_ms < self.floor {
            self.floor = frame_ms;
        } else {
            self.floor = (self.floor * 10f32.powf(0.3 * secs)).min(frame_ms);
        }
        self.floor = self.floor.max(1e-14);
        self.observed_s += secs;
        let hiss_db = 10.0 * self.floor.log10();
        let peak_db = 20.0 * frame_peak.max(1e-9).log10();
        let playing = self.observed_s > SETTLE_S
            && peak_db > (hiss_db + PLAYING_ABOVE_HISS_DB).max(-70.0);
        if !playing {
            return;
        }
        let bin = ((peak_db + 100.0) as usize).min(BINS - 1);
        self.learn_hist[bin] += 1.0;
        // Forget with a ~20 s memory (of playing time).
        let keep = (-secs / 20.0).exp();
        for v in self.recent_hist.iter_mut() {
            *v *= keep;
        }
        self.recent_hist[bin] += 1.0;

        if !self.learned() {
            self.learned_s += secs;
            if let Some(level) = percentile(&self.learn_hist, PERCENTILE) {
                self.gain_db = (REFERENCE_LEVEL_DB - level).clamp(MIN_DB, MAX_DB);
                self.gain.set_target(db_to_gain(self.gain_db));
            }
        } else if let Some(level) = percentile(&self.recent_hist, PERCENTILE) {
            let off = (REFERENCE_LEVEL_DB - level).clamp(MIN_DB, MAX_DB) - self.gain_db;
            if off.abs() > DRIFT_DB {
                self.drift_s += secs;
                if self.drift_s > DRIFT_S {
                    self.relearn();
                }
            } else {
                self.drift_s = 0.0;
            }
        }
    }
}

/// The level (dBFS, bin centre) below which `p` of the counted frames lie.
fn percentile(hist: &[f32; BINS], p: f32) -> Option<f32> {
    let total: f32 = hist.iter().sum();
    if total < 1.0 {
        return None;
    }
    let mut acc = 0.0;
    for (i, v) in hist.iter().enumerate() {
        acc += v;
        if acc >= p * total {
            return Some(i as f32 - 100.0 + 0.5);
        }
    }
    Some(-0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsig::demo_phrase;

    fn hiss(n: usize, db: f32) -> Vec<f32> {
        let g = db_to_gain(db) * 3f32.sqrt();
        let mut s = 9u64;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * g
            })
            .collect()
    }

    /// Hiss is never taken for playing: no matter how noisy the input, an
    /// unplayed guitar gets no boost (boosting hiss is the worst outcome).
    #[test]
    fn hiss_alone_gets_no_boost() {
        let sr = 48_000.0;
        for db in [-80.0f32, -60.0, -50.0, -40.0] {
            let (_, a) = run(&hiss((sr * 10.0) as usize, db), sr);
            assert!(a.gain_db().abs() < 0.01, "hiss at {db} dBFS boosted by {:.1} dB", a.gain_db());
        }
    }

    /// A noisy input with real playing is levelled by the playing, not the hiss.
    #[test]
    fn noisy_input_is_levelled_by_the_playing() {
        let sr = 48_000.0;
        let phrase: Vec<f32> = demo_phrase(sr).iter().map(|v| v * db_to_gain(-18.0)).collect();
        let mut x = hiss((sr * 1.0) as usize + phrase.len() * 2, -58.0);
        let off = (sr * 1.0) as usize;
        for (i, v) in phrase.iter().chain(&phrase).enumerate() {
            x[off + i] += v;
        }
        let (_, a) = run(&x, sr);
        assert!((a.gain_db() - 18.0).abs() < 2.0, "applied {:.1} dB (want ~18)", a.gain_db());
    }

    fn run(x: &[f32], sr: f32) -> (Vec<f32>, AutoLevel) {
        let mut a = AutoLevel::new(sr);
        let mut y = x.to_vec();
        for c in y.chunks_mut(128) {
            a.process(c);
        }
        (y, a)
    }

    /// The voicing input itself needs (almost) no correction.
    #[test]
    fn reference_input_is_left_alone() {
        let sr = 48_000.0;
        let x = demo_phrase(sr);
        let (_, a) = run(&x, sr);
        assert!(a.gain_db().abs() < 1.5, "demo phrase got {:.1} dB", a.gain_db());
    }

    /// The same playing at any input level ends up at the same level.
    #[test]
    fn quiet_and_hot_inputs_are_matched() {
        let sr = 48_000.0;
        let mut x = demo_phrase(sr);
        x.extend(demo_phrase(sr));
        for db in [-24.0f32, -12.0, 6.0] {
            let g = db_to_gain(db);
            let scaled: Vec<f32> = x.iter().map(|v| v * g).collect();
            let (_, a) = run(&scaled, sr);
            assert!(
                (a.gain_db() + db).abs() < 1.5,
                "input {db:+} dB: applied {:.1} dB",
                a.gain_db()
            );
        }
    }

    /// Real playing starts with a light touch of the strings, stray noises,
    /// quiet notes. None of that may set the level: only the playing does.
    #[test]
    fn a_light_touch_first_does_not_set_the_level() {
        let sr = 48_000.0;
        let mut x = hiss((sr * 1.0) as usize, -80.0);
        // A light touch of the strings (-45 dBFS), then two passes of the
        // demo phrase.
        let touch_at = x.len() - (sr * 0.5) as usize;
        for i in 0..(sr * 0.3) as usize {
            let t = i as f32 / sr;
            x[touch_at + i] += db_to_gain(-45.0) * (-t * 20.0).exp() * (2.0 * std::f32::consts::PI * 110.0 * t).sin();
        }
        let phrase = demo_phrase(sr);
        x.extend(phrase.iter().chain(&phrase).map(|v| v * 1.0));
        let (_, a) = run(&x, sr);
        assert!(a.gain_db().abs() < 1.5, "light touch first: applied {:+.1} dB (want ~0)", a.gain_db());
    }
}

