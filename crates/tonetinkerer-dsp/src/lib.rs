//! ToneTinkerer DSP: the complete real-time guitar signal chain.
//!
//! Everything in this crate that runs per sample is allocation-free and
//! lock-free. All allocation happens in constructors / `prepare` calls,
//! which the plugin performs on non-audio threads.

pub mod amp;
pub mod assets;
pub mod biquad;
pub mod cab;
pub mod chain;
pub mod chorus;
pub mod compressor;
pub mod delay;
pub mod eq;
pub mod gate;
pub mod macros;
pub mod overdrive;
pub mod oversample;
pub mod presets;
pub mod resample;
pub mod reverb;
pub mod testsig;
pub mod util;
pub mod wah;

pub use chain::{ChainParams, Engine, GuitarType, InputMode, Meters};
pub use macros::Macros;
