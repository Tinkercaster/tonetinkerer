//! The sound library: complete, ready-to-play rigs grouped by genre.
//!
//! Every sound is an original setting of this plugin's signal chain. Where a
//! sound is *inspired by* a well-known recording, `inspired_by` says so; that
//! is a description of the target character, not a claim that the sound is a
//! reproduction of any artist's rig or recording. Sounds marked "measured"
//! in their description were fitted against reference recordings with the
//! tools in `tools/tone-match` (the recordings are never distributed).
//!
//! Players adjust a sound with the macros (see `macros.rs`); every detailed
//! value below stays reachable in the Advanced view.

use crate::amp::{SLOT_CLEAN, SLOT_CRUNCH, SLOT_LEAD, SLOT_METAL};
use crate::cab::{SLOT_1X12, SLOT_2X12, SLOT_4X12_BRITISH, SLOT_4X12_MODERN};
use crate::chain::ChainParams;
use crate::chorus::ChorusSettings;
use crate::delay::DelaySettings;
use crate::eq::EqSettings;
use crate::reverb::ReverbSettings;
use crate::wah::WahMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Genre {
    Clean,
    BluesJazz,
    Rock,
    Metal,
    Lead,
    Ambient,
}

impl Genre {
    pub const ALL: [Genre; 6] = [
        Genre::Clean,
        Genre::BluesJazz,
        Genre::Rock,
        Genre::Metal,
        Genre::Lead,
        Genre::Ambient,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Genre::Clean => "Clean",
            Genre::BluesJazz => "Blues & Jazz",
            Genre::Rock => "Rock",
            Genre::Metal => "Metal",
            Genre::Lead => "Lead",
            Genre::Ambient => "Ambient",
        }
    }
}

pub struct Preset {
    pub name: &'static str,
    pub genre: Genre,
    /// Shown under the name, e.g. "Metallica – Master of Puppets".
    pub inspired_by: Option<&'static str>,
    pub description: &'static str,
    pub params: fn() -> ChainParams,
}

impl Preset {
    /// Kept for older call sites: the genre label.
    pub fn category(&self) -> &'static str {
        self.genre.label()
    }
}

const fn eq(
    bass_db: f32,
    body_db: f32,
    mid_db: f32,
    mid_freq: f32,
    treble_db: f32,
    presence_db: f32,
) -> EqSettings {
    EqSettings {
        bass_db,
        mid_db,
        mid_freq,
        treble_db,
        presence_db,
        body_db,
    }
}

const fn dly(time_ms: f32, feedback: f32, mix: f32, tone: f32, duck: f32) -> DelaySettings {
    DelaySettings {
        time_ms,
        feedback,
        mix,
        tone,
        ping_pong: true,
        duck,
        modulation: 0.3,
    }
}

const fn rev(decay_s: f32, predelay_ms: f32, damping: f32, mix: f32) -> ReverbSettings {
    ReverbSettings {
        decay_s,
        predelay_ms,
        damping,
        mix,
    }
}

/// Common starting point: gate on, everything else neutral.
fn base() -> ChainParams {
    ChainParams {
        gate_enabled: true,
        // Low on purpose: the gate also closes just above the input's measured
        // hiss (gate.rs), so a sound's threshold only sets how far notes may fade.
        gate_threshold_db: -66.0,
        comp_enabled: true,
        sustain: 0.3,
        drive_enabled: false,
        drive: 0.3,
        drive_tone: 0.5,
        drive_level_db: 2.0,
        drive_fat: false,
        cab_enabled: true,
        cab_low_cut: 70.0,
        cab_high_cut: 10_000.0,
        chorus_enabled: false,
        delay_enabled: false,
        reverb_enabled: true,
        limiter: true,
        ..ChainParams::default()
    }
}

pub const PRESETS: &[Preset] = &[
    // ---------------------------------------------------------------- Clean
    Preset {
        name: "Studio Clean",
        genre: Genre::Clean,
        inspired_by: None,
        description: "Full, open, hi-fi clean that works for everything from chords to arpeggios. A gentle compressor evens out your picking.",
        params: || ChainParams {
            gate_threshold_db: -70.0,
            sustain: 0.25,
            amp_model: SLOT_CLEAN,
            amp_gain_db: -4.0,
            amp_level_db: 10.6,
            cab_ir: SLOT_2X12,
            cab_low_cut: 65.0,
            cab_high_cut: 12_000.0,
            eq: eq(1.0, 1.0, 0.0, 600.0, 1.0, 1.0),
            reverb: rev(1.8, 15.0, 0.4, 0.15),
            ..base()
        },
    },
    Preset {
        name: "Funk Clean",
        genre: Genre::Clean,
        inspired_by: Some("classic funk and disco rhythm guitar"),
        description: "Snappy, squeezed and bright: tight lows, strong compression, quick attack for sixteenth-note scratching.",
        params: || ChainParams {
            gate_threshold_db: -66.0,
            sustain: 0.55,
            amp_model: SLOT_CLEAN,
            amp_gain_db: -6.0,
            amp_level_db: 8.9,
            cab_ir: SLOT_1X12,
            cab_low_cut: 110.0,
            cab_high_cut: 13_000.0,
            eq: eq(-2.0, -1.0, -2.0, 500.0, 3.0, 2.5),
            reverb: rev(0.9, 5.0, 0.4, 0.08),
            ..base()
        },
    },
    Preset {
        name: "80s Chorus Clean",
        genre: Genre::Clean,
        inspired_by: Some("1980s pop and rock clean tones"),
        description: "Shimmering stereo chorus, a dotted-feel echo and a plate-like reverb. Instant arpeggio heaven.",
        params: || ChainParams {
            gate_threshold_db: -68.0,
            sustain: 0.4,
            amp_model: SLOT_CLEAN,
            amp_gain_db: -3.0,
            amp_level_db: 8.8,
            cab_ir: SLOT_2X12,
            cab_low_cut: 80.0,
            cab_high_cut: 12_000.0,
            eq: eq(0.0, 0.0, -1.5, 600.0, 2.0, 1.5),
            chorus_enabled: true,
            chorus: ChorusSettings {
                rate_hz: 0.7,
                depth: 0.6,
                mix: 0.5,
            },
            delay_enabled: true,
            delay: dly(380.0, 0.3, 0.18, 0.5, 0.2),
            reverb: rev(2.6, 20.0, 0.35, 0.22),
            ..base()
        },
    },
    // --------------------------------------------------------- Blues & Jazz
    Preset {
        name: "Jazz Clean",
        genre: Genre::BluesJazz,
        inspired_by: Some("Wes Montgomery – \"Four On Six\""),
        description: "Warm, round and dark: soft top end and a full low-mid voice for octaves and chord melodies. Neck pickup, tone knob rolled back a little.",
        params: || ChainParams {
            gate_threshold_db: -72.0,
            sustain: 0.2,
            amp_model: SLOT_CLEAN,
            amp_gain_db: 4.0,
            amp_level_db: -3.7,
            cab_ir: SLOT_1X12,
            cab_low_cut: 75.0,
            cab_high_cut: 5_900.0,
            eq: eq(7.0, 2.0, 9.0, 385.0, 4.0, 0.5),
            reverb: rev(2.0, 10.0, 0.6, 0.14),
            ..base()
        },
    },
    Preset {
        name: "Smooth Jazz Clean",
        genre: Genre::BluesJazz,
        inspired_by: Some("George Benson – \"Breezin'\""),
        description: "Silky, polished clean with a touch more sparkle than Jazz Clean, gentle compression and a soft, wide room.",
        params: || ChainParams {
            gate_threshold_db: -72.0,
            sustain: 0.45,
            amp_model: SLOT_CLEAN,
            amp_gain_db: 2.0,
            amp_level_db: -3.7,
            cab_ir: SLOT_1X12,
            cab_low_cut: 80.0,
            cab_high_cut: 16_000.0,
            eq: eq(7.0, 1.0, 8.0, 500.0, -1.5, 1.0),
            delay_enabled: true,
            delay: dly(350.0, 0.2, 0.1, 0.4, 0.4),
            reverb: rev(3.5, 25.0, 0.5, 0.2),
            ..base()
        },
    },
    Preset {
        name: "Blues Breakup",
        genre: Genre::BluesJazz,
        inspired_by: Some("classic electric blues (B.B. King style)"),
        description: "A clean amp turned up just past breakup: sweet and singing on single notes, cleans up when you play softly.",
        params: || ChainParams {
            gate_threshold_db: -68.0,
            sustain: 0.35,
            amp_model: SLOT_CLEAN,
            amp_gain_db: 6.0,
            amp_level_db: -0.4,
            cab_ir: SLOT_1X12,
            cab_low_cut: 70.0,
            cab_high_cut: 8_000.0,
            eq: eq(0.5, 1.0, 2.0, 900.0, 1.0, 0.5),
            reverb: rev(2.0, 20.0, 0.45, 0.18),
            ..base()
        },
    },
    Preset {
        name: "Blues Lead",
        genre: Genre::BluesJazz,
        inspired_by: Some("Eric Clapton – \"Before You Accuse Me\""),
        description: "Vocal, mid-rich blues lead with a fat boost into a warm crunch amp. Bends sing, the pick attack stays clear.",
        params: || ChainParams {
            sustain: 0.4,
            drive_enabled: false,
            drive: 0.35,
            drive_tone: 0.4,
            drive_level_db: 3.0,
            drive_fat: true,
            amp_model: SLOT_CRUNCH,
            amp_gain_db: 5.5,
            amp_level_db: -4.0,
            cab_ir: SLOT_4X12_BRITISH,
            cab_low_cut: 55.0,
            cab_high_cut: 16_000.0,
            eq: eq(7.0, 5.0, 4.5, 307.0, 3.0, -3.5),
            delay_enabled: false,
            delay: dly(350.0, 0.2, 0.1, 0.4, 0.4),
            reverb: rev(1.6, 15.0, 0.45, 0.15),
            ..base()
        },
    },
    Preset {
        name: "Blues-Rock Lead",
        genre: Genre::BluesJazz,
        inspired_by: Some("Cream – \"Crossroads\" (1968)"),
        description: "Cranked, thick and dark: the tone rolled back into a hot stack for that smooth, horn-like late-60s lead.",
        params: || ChainParams {
            gate_threshold_db: -66.0,
            sustain: 0.45,
            drive_enabled: true,
            drive: 0.7,
            drive_tone: 0.42,
            drive_level_db: 4.8,
            drive_fat: true,
            amp_model: SLOT_CRUNCH,
            amp_gain_db: -7.0,
            amp_level_db: -3.9,
            cab_ir: SLOT_4X12_BRITISH,
            cab_low_cut: 50.0,
            cab_high_cut: 18_000.0,
            eq: eq(7.0, 5.0, 5.5, 275.0, -3.0, -1.0),
            reverb: rev(1.0, 10.0, 0.5, 0.08),
            ..base()
        },
    },
    // ----------------------------------------------------------------- Rock
    Preset {
        name: "Classic Crunch",
        genre: Genre::Rock,
        inspired_by: Some("classic 70s British rock rhythm"),
        description: "An amp on the edge of breakup: chords ring with grit, dig in for more bite, roll back your volume to clean up.",
        params: || ChainParams {
            comp_enabled: false,
            amp_model: SLOT_CRUNCH,
            amp_gain_db: 1.0,
            amp_level_db: 2.8,
            cab_ir: SLOT_4X12_BRITISH,
            cab_low_cut: 80.0,
            cab_high_cut: 11_000.0,
            eq: eq(0.5, 0.5, 5.0, 700.0, 0.5, 1.0),
            reverb: rev(1.0, 10.0, 0.5, 0.07),
            ..base()
        },
    },
    Preset {
        name: "Hard Rock",
        genre: Genre::Rock,
        inspired_by: Some("80s hard rock rhythm and lead"),
        description: "A boosted, cranked stack: big, punchy power chords with enough gain for solos.",
        params: || ChainParams {
            gate_threshold_db: -66.0,
            comp_enabled: false,
            drive_enabled: true,
            drive: 0.25,
            drive_tone: 0.55,
            drive_level_db: 5.0,
            amp_model: SLOT_CRUNCH,
            amp_gain_db: 6.0,
            amp_level_db: -1.4,
            cab_ir: SLOT_4X12_BRITISH,
            cab_low_cut: 85.0,
            cab_high_cut: 9_750.0,
            eq: eq(1.0, 0.5, 6.0, 800.0, 4.5, 1.0),
            delay_enabled: true,
            delay: dly(300.0, 0.15, 0.08, 0.5, 0.4),
            reverb: rev(1.2, 10.0, 0.5, 0.08),
            ..base()
        },
    },
    Preset {
        name: "Rhythm Crunch",
        genre: Genre::Rock,
        inspired_by: None,
        description: "Classic cranked-British crunch for riffs and power chords. Dry and tight.",
        params: || ChainParams {
            gate_threshold_db: -66.0,
            comp_enabled: false,
            amp_model: SLOT_CRUNCH,
            amp_gain_db: 3.0,
            amp_level_db: 2.3,
            cab_ir: SLOT_4X12_BRITISH,
            cab_low_cut: 88.0,
            cab_high_cut: 12_000.0,
            eq: eq(0.5, 0.0, 5.0, 700.0, 2.0, -1.0),
            reverb: rev(1.0, 10.0, 0.5, 0.06),
            ..base()
        },
    },
    // ---------------------------------------------------------------- Metal
    Preset {
        name: "Thrash Rhythm",
        genre: Genre::Metal,
        inspired_by: Some("Metallica – \"Master of Puppets\""),
        description: "Scooped, tight and aggressive: a clean boost tightens the low end so fast palm-muted downpicking stays articulate.",
        params: || ChainParams {
            gate_threshold_db: -66.0,
            comp_enabled: false,
            drive_enabled: true,
            drive: 0.5,
            drive_tone: 0.15,
            drive_level_db: -3.0,
            amp_model: SLOT_METAL,
            amp_gain_db: 5.0,
            amp_level_db: 3.0,
            cab_ir: SLOT_4X12_BRITISH,
            cab_low_cut: 50.0,
            cab_high_cut: 18_000.0,
            eq: eq(8.0, 5.0, 1.0, 270.0, 1.5, -2.0),
            reverb: rev(1.2, 5.0, 0.5, 0.05),
            ..base()
        },
    },
    Preset {
        name: "Modern Metal",
        genre: Genre::Metal,
        inspired_by: Some("Lamb of God – \"Laid to Rest\""),
        description: "Ultra-tight, punchy modern high gain with an active-pickup feel: fast attack, solid low end, cutting upper mids. Great for drop tunings.",
        params: || ChainParams {
            gate_threshold_db: -66.0,
            comp_enabled: false,
            drive_enabled: true,
            drive: 0.65,
            drive_tone: 0.8,
            drive_level_db: 7.0,
            amp_model: SLOT_METAL,
            amp_gain_db: 5.0,
            amp_level_db: 0.0,
            cab_ir: SLOT_4X12_MODERN,
            cab_low_cut: 60.0,
            cab_high_cut: 18_000.0,
            eq: eq(8.0, 5.0, 7.5, 266.0, 4.0, -1.0),
            reverb: rev(0.8, 5.0, 0.5, 0.03),
            ..base()
        },
    },
    Preset {
        name: "Heavy Riff",
        genre: Genre::Metal,
        inspired_by: Some("Metallica – \"Some Kind of Monster\""),
        description: "Thick, raw and heavy: more low end and body than Thrash Rhythm, for big, slow, down-tuned riffs.",
        params: || ChainParams {
            gate_threshold_db: -66.0,
            comp_enabled: false,
            amp_model: SLOT_METAL,
            amp_gain_db: 5.0,
            amp_level_db: 2.5,
            cab_ir: SLOT_4X12_MODERN,
            cab_low_cut: 52.0,
            cab_high_cut: 18_000.0,
            eq: eq(8.0, 5.0, 4.0, 266.0, -2.0, -3.5),
            reverb: rev(1.0, 5.0, 0.5, 0.05),
            drive_enabled: true,
            drive: 0.65,
            drive_tone: 0.8,
            drive_level_db: -4.0,
            ..base()
        },
    },
    Preset {
        name: "Metal Lead",
        genre: Genre::Metal,
        inspired_by: None,
        description: "Saturated, singing high-gain lead with pushed mids so solos cut through a heavy mix.",
        params: || ChainParams {
            gate_threshold_db: -66.0,
            sustain: 0.45,
            drive_enabled: true,
            drive: 0.3,
            drive_tone: 0.5,
            drive_level_db: 3.0,
            amp_model: SLOT_METAL,
            amp_gain_db: 4.0,
            amp_level_db: 2.0,
            cab_ir: SLOT_4X12_MODERN,
            cab_low_cut: 75.0,
            cab_high_cut: 9_400.0,
            eq: eq(0.0, 1.0, 4.0, 900.0, 4.0, 0.0),
            delay_enabled: true,
            delay: dly(380.0, 0.3, 0.18, 0.45, 0.45),
            reverb: rev(2.0, 20.0, 0.45, 0.15),
            ..base()
        },
    },
    // ----------------------------------------------------------------- Lead
    Preset {
        name: "Singing Lead",
        genre: Genre::Lead,
        inspired_by: Some("Joe Satriani – \"Always With Me, Always With You\""),
        description: "Warm, vocal and endlessly sustained, with an open top: the classic instrumental-rock ballad lead. Measured against its reference. Neck pickup recommended.",
        params: || ChainParams {
            sustain: 0.55,
            drive_enabled: true,
            drive: 0.3,
            drive_tone: 0.2,
            drive_level_db: 7.0,
            drive_fat: true,
            amp_model: SLOT_CRUNCH,
            amp_gain_db: 5.0,
            amp_level_db: 2.1,
            cab_ir: SLOT_4X12_MODERN,
            cab_low_cut: 70.0,
            cab_high_cut: 11_500.0,
            eq: eq(8.0, 0.0, -5.0, 1_160.0, 6.5, 1.0),
            delay_enabled: true,
            delay: dly(430.0, 0.4, 0.22, 0.45, 0.5),
            reverb: rev(5.0, 35.0, 0.45, 0.25),
            ..base()
        },
    },
    Preset {
        name: "Emotive Lead",
        genre: Genre::Lead,
        inspired_by: Some("Joe Satriani – \"Until We Say Goodbye\""),
        description: "Round and warm with a full low-mid voice, generous echo and a long hall. Measured against its reference.",
        params: || ChainParams {
            sustain: 0.55,
            drive_enabled: true,
            drive: 0.5,
            drive_tone: 0.15,
            drive_level_db: 5.0,
            drive_fat: true,
            amp_model: SLOT_CRUNCH,
            amp_gain_db: -3.0,
            amp_level_db: -2.5,
            cab_ir: SLOT_4X12_MODERN,
            cab_low_cut: 50.0,
            cab_high_cut: 16_000.0,
            eq: eq(8.0, 4.0, 3.0, 270.0, 4.5, 1.5),
            delay_enabled: true,
            delay: dly(400.0, 0.4, 0.25, 0.45, 0.45),
            reverb: rev(5.0, 40.0, 0.5, 0.28),
            ..base()
        },
    },
    Preset {
        name: "Vocal Wah Lead",
        genre: Genre::Lead,
        inspired_by: Some("Joe Satriani – \"Cryin'\""),
        description: "A half-cocked wah adds a human, vowel-like voice over a warm, heavily sustained tone for long bends. Measured against its reference.",
        params: || ChainParams {
            wah_mode: WahMode::Manual,
            wah_position: 0.58,
            sustain: 0.65,
            drive_enabled: true,
            drive: 0.6,
            drive_tone: 0.7,
            drive_level_db: 0.0,
            drive_fat: true,
            amp_model: SLOT_CRUNCH,
            amp_gain_db: 5.0,
            amp_level_db: -2.4,
            cab_ir: SLOT_4X12_BRITISH,
            cab_low_cut: 50.0,
            cab_high_cut: 11_500.0,
            eq: eq(8.0, 4.0, 7.0, 285.0, -3.5, 0.0),
            delay_enabled: true,
            delay: dly(510.0, 0.35, 0.2, 0.45, 0.45),
            reverb: rev(4.5, 30.0, 0.45, 0.24),
            ..base()
        },
    },
    Preset {
        name: "Wah Shred",
        genre: Genre::Lead,
        inspired_by: Some("Joe Satriani – \"Surfing with the Alien\""),
        description: "High gain with an envelope wah that quacks with every pick attack. Switch the wah to Manual in Advanced and map Wah Position to an expression pedal for real sweeps.",
        params: || ChainParams {
            gate_threshold_db: -66.0,
            wah_mode: WahMode::Auto,
            wah_position: 0.3,
            wah_sensitivity: 0.6,
            sustain: 0.45,
            drive_enabled: true,
            drive: 0.6,
            drive_tone: 0.6,
            drive_level_db: 3.0,
            amp_model: SLOT_LEAD,
            amp_gain_db: 2.0,
            amp_level_db: 1.5,
            cab_ir: SLOT_4X12_MODERN,
            cab_low_cut: 75.0,
            cab_high_cut: 10_600.0,
            eq: eq(0.5, 0.0, 3.5, 1_000.0, 4.5, 1.0),
            delay_enabled: true,
            delay: dly(300.0, 0.25, 0.14, 0.55, 0.35),
            reverb: rev(1.8, 15.0, 0.4, 0.1),
            ..base()
        },
    },
    Preset {
        name: "Smooth Lead",
        genre: Genre::Lead,
        inspired_by: None,
        description: "A fat boost pushes the lead channel into effortless, warm sustain. A great all-round solo sound.",
        params: || ChainParams {
            sustain: 0.55,
            drive_enabled: true,
            drive: 0.45,
            drive_tone: 0.3,
            drive_level_db: 2.0,
            drive_fat: true,
            amp_model: SLOT_LEAD,
            amp_gain_db: -4.0,
            amp_level_db: -0.3,
            cab_ir: SLOT_2X12,
            cab_low_cut: 65.0,
            cab_high_cut: 10_500.0,
            eq: eq(3.0, 0.0, 3.0, 600.0, 1.0, 1.5),
            delay_enabled: true,
            delay: dly(420.0, 0.35, 0.2, 0.45, 0.45),
            reverb: rev(3.5, 25.0, 0.45, 0.18),
            ..base()
        },
    },
    Preset {
        name: "High-Gain Lead",
        genre: Genre::Lead,
        inspired_by: None,
        description: "Saturated, articulate high-gain lead: tight lows, cutting upper mids, short slap echo.",
        params: || ChainParams {
            gate_threshold_db: -66.0,
            sustain: 0.45,
            drive_enabled: true,
            drive: 0.55,
            drive_tone: 0.55,
            drive_level_db: 3.0,
            amp_model: SLOT_LEAD,
            amp_gain_db: 2.0,
            amp_level_db: 1.9,
            cab_ir: SLOT_4X12_MODERN,
            cab_low_cut: 80.0,
            cab_high_cut: 10_250.0,
            eq: eq(0.0, 0.0, 4.0, 1_100.0, 4.5, 1.0),
            delay_enabled: true,
            delay: dly(330.0, 0.22, 0.12, 0.55, 0.3),
            reverb: rev(1.5, 15.0, 0.4, 0.1),
            ..base()
        },
    },
    // -------------------------------------------------------------- Ambient
    Preset {
        name: "Ambient Lead",
        genre: Genre::Ambient,
        inspired_by: None,
        description: "Spacious stereo lead: wide ping-pong echoes that duck while you play and a long, lush hall.",
        params: || ChainParams {
            sustain: 0.6,
            drive_enabled: true,
            drive: 0.4,
            drive_tone: 0.3,
            drive_level_db: 2.0,
            drive_fat: true,
            amp_model: SLOT_CRUNCH,
            amp_gain_db: 2.0,
            amp_level_db: -3.3,
            cab_ir: SLOT_2X12,
            cab_low_cut: 65.0,
            cab_high_cut: 10_000.0,
            eq: eq(3.0, 0.0, 2.5, 500.0, 0.5, 1.0),
            delay_enabled: true,
            delay: DelaySettings {
                time_ms: 620.0,
                feedback: 0.55,
                mix: 0.35,
                tone: 0.4,
                ping_pong: true,
                duck: 0.5,
                modulation: 0.5,
            },
            reverb: rev(7.0, 60.0, 0.5, 0.35),
            ..base()
        },
    },
    Preset {
        name: "Dreamy Swells",
        genre: Genre::Ambient,
        inspired_by: None,
        description: "A soft clean washed in slow chorus, long repeats and a huge reverb. Use your volume knob for swells.",
        params: || ChainParams {
            gate_threshold_db: -72.0,
            sustain: 0.5,
            amp_model: SLOT_CLEAN,
            amp_gain_db: -2.0,
            amp_level_db: 4.9,
            cab_ir: SLOT_2X12,
            cab_low_cut: 70.0,
            cab_high_cut: 9_000.0,
            eq: eq(1.0, 1.0, 0.0, 600.0, -1.0, 0.0),
            chorus_enabled: true,
            chorus: ChorusSettings {
                rate_hz: 0.3,
                depth: 0.7,
                mix: 0.4,
            },
            delay_enabled: true,
            delay: DelaySettings {
                time_ms: 600.0,
                feedback: 0.55,
                mix: 0.4,
                tone: 0.4,
                ping_pong: true,
                duck: 0.3,
                modulation: 0.5,
            },
            reverb: rev(8.0, 80.0, 0.5, 0.45),
            ..base()
        },
    },
];

pub fn find(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_names_are_unique_and_values_in_range() {
        for (i, a) in PRESETS.iter().enumerate() {
            for b in &PRESETS[i + 1..] {
                assert_ne!(a.name, b.name);
            }
            let p = (a.params)();
            assert!((0.0..=1.0).contains(&p.sustain));
            assert!((0.0..=1.0).contains(&p.drive));
            assert!((0.0..=0.98).contains(&p.delay.feedback));
            assert!((0.0..=1.0).contains(&p.reverb.mix));
            assert!(p.eq.mid_freq >= 200.0 && p.eq.mid_freq <= 4_000.0);
        }
    }

    #[test]
    fn every_genre_has_sounds() {
        for g in Genre::ALL {
            assert!(
                PRESETS.iter().filter(|p| p.genre == g).count() >= 2,
                "{}",
                g.label()
            );
        }
    }
}
