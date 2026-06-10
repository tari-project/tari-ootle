//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_main]
//! Wire/mempool transaction decode — exercises the alloc-from-length sink.
//!
//! Mirrors the p2p `decode_from_slice` -> `tari_bor::decode_exact` path used by
//! RPC and mempool gossip. `Transaction` decode walks the `tari_bor` collection
//! adapters (`Vec`/`IndexSet`/`IndexMap::with_capacity(n)` over `IndexSet<SubstateRequirement>`
//! inputs, nested `Evidence` maps, access-rule lists, ...), where `n` is the raw
//! CBOR length header with no bound against the remaining input — a ~10-byte
//! payload can request a multi-GB allocation.
//!
//! Run with allocation limits so the failure surfaces as a crash artifact:
//!   cargo fuzz run transaction_decode -- -rss_limit_mb=512 -malloc_limit_mb=64

use libfuzzer_sys::fuzz_target;
use tari_ootle_transaction::Transaction;

fuzz_target!(|data: &[u8]| {
    let _ = tari_bor::decode_exact::<Transaction>(data);
});
