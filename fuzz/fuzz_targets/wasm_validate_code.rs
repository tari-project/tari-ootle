//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_main]
//! Untrusted WASM template binary — `WasmModule::validate_code`.
//!
//! Validation runs the wasmer compiler plus the custom-section + ABI-decode
//! paths over an attacker-supplied module. We bail above the engine's binary
//! size limit (the production caller rejects oversized modules before this
//! point) so the fuzzer stays in the interesting region.
//!
//! Seed corpus is a real compiled template (`fuzz/seeds/wasm_validate_code`)
//! so libFuzzer mutates around a valid module and reaches past the magic-number
//! gate. Run with resource limits, e.g.:
//!   cargo fuzz run wasm_validate_code -- -rss_limit_mb=2048 -timeout=25

use libfuzzer_sys::fuzz_target;
use tari_engine::wasm::WasmModule;
use tari_engine_types::limits;

fuzz_target!(|data: &[u8]| {
    if data.len() > limits::ENGINE_LIMITS.max_template_binary_size_bytes {
        return;
    }
    let _ = WasmModule::validate_code(data);
});
