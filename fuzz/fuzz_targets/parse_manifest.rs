//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_main]
//! Manifest DSL parser — stack overflow via nested token groups.
//!
//! Deeply nested `()`/`{}`/`[]` overflows the stack during the syn/proc_macro2
//! lex/parse AND again on `Drop` of the resulting token tree. We run on a small
//! bounded stack so the overflow surfaces as a reproducible crash artifact, and
//! cap input size so the fuzzer spends its time on structure rather than length.

use std::collections::HashMap;

use libfuzzer_sys::fuzz_target;
use tari_transaction_manifest::{parse_manifest, ManifestValue};

const MAX_INPUT_BYTES: usize = 5 * 1024 * 1024;
const FUZZ_STACK_BYTES: usize = 256 * 1024;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    if src.len() > MAX_INPUT_BYTES {
        return;
    }

    let src = src.to_string();
    let handle = std::thread::Builder::new()
        .stack_size(FUZZ_STACK_BYTES)
        .spawn(move || {
            let mut globals = HashMap::new();
            if let Ok(v) = "044bccd4d01ceb41816bc9106a836806e6f9412646ecda4c2d726d8372b2c843".parse::<ManifestValue>() {
                globals.insert("owner".to_string(), v);
            }
            let _ = parse_manifest(&src, globals, Default::default(), Default::default());
        })
        .expect("spawn bounded-stack fuzz thread");
    let _ = handle.join();
});
