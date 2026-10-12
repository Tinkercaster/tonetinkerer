// Optional skin artwork: if assets/skin/background.png exists it is embedded
// and drawn behind the UI (see src/skin.rs). No file -> procedural background.
use std::path::Path;

fn main() {
    let bg = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/skin/background.png");
    println!("cargo::rustc-check-cfg=cfg(skin_bg)");
    println!("cargo:rerun-if-changed={}", bg.display());
    println!("cargo:rerun-if-changed=../../assets/skin");
    if bg.exists() {
        println!("cargo:rustc-cfg=skin_bg");
    }
}
