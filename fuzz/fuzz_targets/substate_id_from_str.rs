//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_main]
//! Address / substate string parsing — regression guard for the hex panic.
//!
//! Mirrors the wallet daemon's `submit_manifest` variable-parse boundary:
//! attacker-controlled `variables` strings flow through `ManifestValue::from_str`
//! -> `SubstateId::from_str` -> `*::from_hex`. A non-ASCII byte straddling a
//! 2-byte hex chunk boundary used to panic in `fixed_bytes_from_hex`/
//! `bytes_from_hex` (`crates/template_lib_types/src/hex.rs`); this target keeps
//! that fixed across every address type.
//!
//! Repro seed that crashed before the hex.rs fix:
//!   format!("nft_{}{}_u32_1", '\u{1F600}', "a".repeat(60))

use std::str::FromStr;

use libfuzzer_sys::fuzz_target;
use tari_engine_types::substate::SubstateId;
use tari_template_lib_types::NonFungibleAddress;
use tari_transaction_manifest::ManifestValue;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let _ = SubstateId::from_str(s);
    let _ = NonFungibleAddress::from_str(s);
    // The real walletd entry point: tries SubstateId, then NonFungibleId, then
    // raw hex bytes — covers the hex sink directly.
    let _ = ManifestValue::from_str(s);
});
