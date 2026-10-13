//! Bundled amp models and cabinet IRs, compiled into the binary so the
//! plugin works with zero setup. All are original works (see ASSETS.md):
//! the NAM models are virtual captures of an in-house reference circuit, the
//! first three IRs are synthesized speaker responses, and the two "4x12 Modern"
//! / "4x12 British" IRs are real microphone recordings released under CC0.

use crate::amp::{self, NamSlot};
use crate::cab::{self, CabSlot};
use crate::chain::{Engine, MAX_CHUNK};
use nam_rs::NamModel;
use std::sync::OnceLock;

pub const AMP_MODELS: [(&str, &[u8]); 4] = [
    ("Clean", include_bytes!("../../../assets/models/clean.nam")),
    (
        "Crunch",
        include_bytes!("../../../assets/models/crunch.nam"),
    ),
    ("Lead", include_bytes!("../../../assets/models/lead.nam")),
    ("Metal", include_bytes!("../../../assets/models/metal.nam")),
];

pub const CAB_IRS: [(&str, &[u8]); 5] = [
    (
        "4x12 Closed",
        include_bytes!("../../../assets/irs/4x12_closed.wav"),
    ),
    (
        "2x12 Open",
        include_bytes!("../../../assets/irs/2x12_open.wav"),
    ),
    (
        "1x12 Vintage",
        include_bytes!("../../../assets/irs/1x12_vintage.wav"),
    ),
    (
        "4x12 Modern",
        include_bytes!("../../../assets/irs/4x12_modern.wav"),
    ),
    (
        "4x12 British",
        include_bytes!("../../../assets/irs/4x12_british.wav"),
    ),
];

static PARSED: [OnceLock<NamModel>; 4] = [
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
];

/// Parsed bundled model (parsed once per process, then cached).
pub fn bundled_nam(idx: usize) -> &'static NamModel {
    PARSED[idx].get_or_init(|| {
        let json = std::str::from_utf8(AMP_MODELS[idx].1).expect("bundled model is UTF-8");
        NamModel::from_json_str(json).expect("bundled model parses")
    })
}

/// Build a complete engine with all bundled models and IRs installed.
/// Allocates and parses: call off the audio thread.
pub fn build_engine(sample_rate: f32) -> Engine {
    let mut e = Engine::new(sample_rate);
    let lat = e.amp_latency();
    for (i, (name, _)) in AMP_MODELS.iter().enumerate() {
        let slot = NamSlot::new(bundled_nam(i), name, sample_rate as f64, MAX_CHUNK, lat)
            .expect("bundled model builds");
        e.amp.set_slot(
            [
                amp::SLOT_CLEAN,
                amp::SLOT_CRUNCH,
                amp::SLOT_LEAD,
                amp::SLOT_METAL,
            ][i],
            Some(Box::new(slot)),
        );
    }
    for (i, (name, bytes)) in CAB_IRS.iter().enumerate() {
        let slot = CabSlot::from_wav_bytes(bytes, name, sample_rate).expect("bundled IR loads");
        e.cab.set_slot(
            [
                cab::SLOT_4X12,
                cab::SLOT_2X12,
                cab::SLOT_1X12,
                cab::SLOT_4X12_MODERN,
                cab::SLOT_4X12_BRITISH,
            ][i],
            Some(Box::new(slot)),
        );
    }
    e
}

/// Build a custom amp slot compatible with `engine` (off the audio thread).
pub fn custom_amp_slot(
    path: &std::path::Path,
    sample_rate: f32,
    amp_latency: usize,
) -> Result<NamSlot, amp::AmpError> {
    NamSlot::from_file(path, sample_rate as f64, MAX_CHUNK, amp_latency)
}
