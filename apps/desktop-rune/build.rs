//! Embeds the Rust crate notices CI generates (`tools/crate_notices.py`) for the Open source
//! licenses page. Builds without `S5_CRATE_NOTICES` embed an empty manifest.
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=S5_CRATE_NOTICES");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("crate-notices.json");
    match std::env::var("S5_CRATE_NOTICES").ok().filter(|p| !p.is_empty()) {
        Some(path) => {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
            println!("cargo:rerun-if-changed={}", path.display());
            std::fs::copy(&path, &out).unwrap_or_else(|e| panic!("S5_CRATE_NOTICES {}: {e}", path.display()));
        }
        None => std::fs::write(&out, r#"{"schemaVersion":1,"entries":[]}"#).unwrap(),
    }
}
