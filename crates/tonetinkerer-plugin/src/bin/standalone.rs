//! Standalone ToneTinkerer (development aid: run the editor without a DAW).
fn main() {
    nice_plug::prelude::nice_export_standalone::<tonetinkerer_plugin::ToneTinkerer>();
}
