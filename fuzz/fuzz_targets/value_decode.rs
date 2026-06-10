//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![no_main]
//! CBOR recursion sink — the highest-leverage harness.
//!
//! `tari_bor::Value::decode` (and everything that funnels into it: p2p wire
//! decode, transaction/substate decode, the WASM host-call arg path) recurses
//! through `decode_array`/`decode_map`/the `Tag` arm with no depth limit. The
//! `MAX_VISITOR_DEPTH` guard in `walker.rs` only runs *after* the tree is fully
//! materialised, so it does not protect the decode itself — roughly one input
//! byte per nesting level overflows the stack.
//!
//! We run the decode on a bounded stack so any *un*bounded recursion surfaces
//! as a reproducible crash artifact at a shallow, deterministic depth instead
//! of depending on the (large) default libFuzzer thread stack. The stack is
//! sized comfortably above what a correctly depth-capped decode
//! (`tari_bor::MAX_DECODE_DEPTH`) needs, so a clean run fits while a regression
//! that removes the cap still overflows within a few hundred bytes of input.
//!
//! Seeds that overflow without the cap: chains of `0x81` (array-of-1), `0x9f`
//! (indef array), `0xc0` (tag).

use libfuzzer_sys::fuzz_target;

const FUZZ_STACK_BYTES: usize = 512 * 1024;

fuzz_target!(|data: &[u8]| {
    let data = data.to_vec();
    let handle = std::thread::Builder::new()
        .stack_size(FUZZ_STACK_BYTES)
        .spawn(move || {
            let _ = tari_bor::decode::<tari_bor::Value>(&data);
            let _ = tari_engine_types::substate::SubstateValue::from_bytes(&data);
        })
        .expect("spawn bounded-stack fuzz thread");
    // A stack overflow aborts the whole process (caught by libFuzzer). Otherwise
    // propagate any worker panic so libFuzzer records it as a crash too.
    handle.join().expect("decode worker panicked");
});
