//! Detect the optional dApp pack at build time.
//!
//! When `src/dapp/mod.rs` is present, scanners are compiled and the UI module
//! at `static/dapp/mod.js` is embedded. When the folder is removed, the binary
//! still builds with an empty registry and the frontend skips the UI pack.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/dapp/mod.rs");
    println!("cargo:rerun-if-changed=static/dapp/mod.js");
    println!("cargo:rustc-check-cfg=cfg(has_dapp)");

    if Path::new("src/dapp/mod.rs").exists() {
        if !Path::new("static/dapp/mod.js").exists() {
            panic!("src/dapp is present but static/dapp/mod.js is missing");
        }
        println!("cargo:rustc-cfg=has_dapp");
    }
}
