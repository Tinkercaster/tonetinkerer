//! Plain-language "macro" controls.
//!
//! Each macro moves several detailed parameters at once, relative to the
//! selected sound: 0 is the sound exactly as designed, -1 / +1 the extremes.
//! `apply` is a pure function from (sound settings, macros) to the effective
//! settings, so the plugin and the offline renderer behave identically and
//! all smoothing still happens inside the DSP.

use crate::chain::ChainParams;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Macros {
    /// Clean <-> saturated: amp gain, drive amount, gate threshold.
    pub gain: f32,
    /// Thin <-> thick: bass, low-mid body, low cut.
    pub body: f32,
    /// Dark <-> bright: treble, presence, high cut, drive tone.
    pub bright: f32,
    /// Scooped <-> pushed: mid band at the sound's own mid frequency.
    pub mids: f32,
    /// Less <-> more sustain: compressor amount and a little extra gain.
    pub sustain: f32,
    /// Dry <-> huge: delay + reverb amount, reverb length, echo repeats.
    pub space: f32,
    /// Output level change in dB.
    pub volume_db: f32,
}

impl Macros {
    pub fn is_neutral(&self) -> bool {
        *self == Macros::default()
    }
}

/// Parameter ranges the macros must respect (same as the plugin's host parameters).
pub mod limits {
    pub const AMP_GAIN: (f32, f32) = (-18.0, 18.0);
    pub const EQ: (f32, f32) = (-12.0, 12.0);
    pub const GATE: (f32, f32) = (-96.0, -20.0);
    pub const LOW_CUT: (f32, f32) = (20.0, 400.0);
    pub const HIGH_CUT: (f32, f32) = (2_000.0, 20_000.0);
    pub const DECAY: (f32, f32) = (0.3, 12.0);
    pub const FEEDBACK: (f32, f32) = (0.0, 0.95);
    pub const OUTPUT: (f32, f32) = (-24.0, 12.0);
}

fn clamp(v: f32, r: (f32, f32)) -> f32 {
    v.clamp(r.0, r.1)
}

/// Effective settings for `base` with macros `m` applied.
pub fn apply(base: &ChainParams, m: &Macros) -> ChainParams {
    let mut p = base.clone();
    if m.is_neutral() {
        return p;
    }
    let c = |x: f32| x.clamp(-1.0, 1.0);
    let (g, b, br, mi, su, sp) = (
        c(m.gain),
        c(m.body),
        c(m.bright),
        c(m.mids),
        c(m.sustain),
        c(m.space),
    );
    use limits::*;

    // Gain.
    p.amp_gain_db = clamp(p.amp_gain_db + 9.0 * g + 3.0 * su.max(0.0), AMP_GAIN);
    p.drive = (p.drive + 0.25 * g).clamp(0.0, 1.0);
    p.gate_threshold_db = clamp(p.gate_threshold_db + 6.0 * g, GATE);

    // Body.
    p.eq.bass_db = clamp(p.eq.bass_db + 5.0 * b, EQ);
    p.eq.body_db = clamp(p.eq.body_db + 5.0 * b, EQ);
    p.cab_low_cut = clamp(p.cab_low_cut * 2f32.powf(-0.7 * b), LOW_CUT);

    // Bright.
    p.eq.treble_db = clamp(p.eq.treble_db + 5.0 * br, EQ);
    p.eq.presence_db = clamp(p.eq.presence_db + 3.0 * br, EQ);
    p.cab_high_cut = clamp(
        p.cab_high_cut * 2f32.powf(0.7 * br),
        (4_000.0f32.max(HIGH_CUT.0), 18_000.0f32.min(HIGH_CUT.1)),
    );
    p.drive_tone = (p.drive_tone + 0.2 * br).clamp(0.0, 1.0);

    // Mids.
    p.eq.mid_db = clamp(p.eq.mid_db + 7.0 * mi, EQ);

    // Sustain.
    if su > 0.0 {
        p.comp_enabled = true;
    }
    p.sustain = (p.sustain + 0.4 * su).clamp(0.0, 1.0);

    // Space.
    if sp < 0.0 {
        let k = 1.0 + sp;
        p.delay.mix *= k;
        p.reverb.mix *= k;
    } else if sp > 0.0 {
        let k = 1.0 + 1.5 * sp;
        // A sound designed fully dry still gets some space when asked for.
        p.delay.mix = (p.delay.mix.max(0.08) * k).min(1.0);
        p.reverb.mix = (p.reverb.mix.max(0.08) * k).min(1.0);
        p.delay_enabled = true;
        p.reverb_enabled = true;
        p.reverb.decay_s = clamp(p.reverb.decay_s * 2f32.powf(0.6 * sp), DECAY);
        p.delay.feedback = clamp(p.delay.feedback + 0.15 * sp, FEEDBACK);
    }

    // Volume.
    p.output_db = clamp(p.output_db + m.volume_db, OUTPUT);
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::PRESETS;

    #[test]
    fn neutral_is_identity() {
        for preset in PRESETS {
            let base = (preset.params)();
            assert_eq!(apply(&base, &Macros::default()), base);
        }
    }

    #[test]
    fn extremes_stay_in_range() {
        for preset in PRESETS {
            let base = (preset.params)();
            for v in [-1.0f32, 1.0] {
                for k in 0..6 {
                    let mut m = Macros::default();
                    match k {
                        0 => m.gain = v,
                        1 => m.body = v,
                        2 => m.bright = v,
                        3 => m.mids = v,
                        4 => m.sustain = v,
                        _ => m.space = v,
                    }
                    let p = apply(&base, &m);
                    for f in p.tone_fields() {
                        assert!(f.is_finite());
                    }
                    assert!((limits::EQ.0..=limits::EQ.1).contains(&p.eq.body_db));
                    assert!((limits::HIGH_CUT.0..=limits::HIGH_CUT.1).contains(&p.cab_high_cut));
                    assert!(
                        (0.0..=1.0).contains(&p.delay.mix) && (0.0..=1.0).contains(&p.reverb.mix)
                    );
                }
            }
        }
    }

    #[test]
    fn space_fully_down_is_dry() {
        let base = (PRESETS[0].params)();
        let p = apply(
            &base,
            &Macros {
                space: -1.0,
                ..Default::default()
            },
        );
        assert_eq!(p.delay.mix, 0.0);
        assert_eq!(p.reverb.mix, 0.0);
    }
}
