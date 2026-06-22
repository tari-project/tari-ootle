//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Generates `include/ootle_sdk.h` from the `extern "C"` surface via cbindgen.
//!
//! The header is **committed** at `include/ootle_sdk.h` (the stable cross-repo contract the Go SDK
//! vendors) and regenerated on every build so it can never silently drift from the Rust source. The
//! generation is best-effort: a cbindgen failure logs a warning rather than failing the build, so a
//! consumer that only needs the compiled lib (and vendors the committed header) is never blocked.

use std::{env, path::PathBuf};

fn main() {
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let out_header = crate_dir.join("include").join("ootle_sdk.h");

    // Rerun if the surface or config changes.
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/c_abi.rs");
    println!("cargo:rerun-if-changed=src/stealth_abi.rs");
    println!("cargo:rerun-if-changed=src/substate_decode_abi.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");

    let config = match cbindgen::Config::from_file(crate_dir.join("cbindgen.toml")) {
        Ok(config) => config,
        Err(e) => {
            println!("cargo:warning=cbindgen: could not read cbindgen.toml: {e}");
            return;
        },
    };

    match cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
    {
        Ok(bindings) => {
            bindings.write_to_file(&out_header);
        },
        Err(e) => {
            // Don't fail the build — the committed header remains the source of truth for consumers.
            println!("cargo:warning=cbindgen: header generation failed ({e}); using committed include/ootle_sdk.h");
        },
    }
}
