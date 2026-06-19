# `ootle_sdk_core`

The **pure, synchronous fat core** for Tari Ootle SDKs.

It is a pure function of developer intent → submit-ready BOR-encoded transaction bytes (and the
inverse for results). Host SDKs (Go via cbindgen, Kotlin/Swift via UniFFI, TS/Python via WASM)
wrap this crate; all I/O, async, and binding-generator concerns live in those hosts, never here.

## The hard rule — forbidden dependencies (Decision D1/D2)

This crate must have **no** `tokio`, `reqwest`, `async`/`await`, `wasm-bindgen`, `uniffi`, or
`cbindgen` anywhere in its source **or its dependency tree**. It is generator-agnostic: zero
`#[uniffi(...)]` / `#[wasm_bindgen]` / cbindgen attributes on any type. Boundary types are plain
Rust. `#![forbid(unsafe_code)]` at the crate root.

If you reach for one of the forbidden deps, you have crossed the boundary — **stop and reconsider**;
the work belongs in a host or a later facade.

Verify purity with:

```sh
cargo tree -p ootle_sdk_core | grep -E 'tokio|reqwest|uniffi|cbindgen|wasm-bindgen'
```

It must print nothing.

## Reuse, don't re-port

The orchestration this crate wires already exists as **pure** code elsewhere in the workspace
(`tari_ootle_transaction`'s `TransactionBuilder`, the seal/sign/encode leaves, and
`ootle-wasm-core`'s pure helpers). **Find the existing thing and depend on it.** The core's new code
is *orchestration + the typed boundary* — not a re-implementation of shared logic.

## What works today (Phase 0 — closed)

A **public transfer** goes from typed intent → submit-ready BOR-encoded bytes, fully deterministically:

```rust
use ootle_sdk_core::{
    build_and_encode_public_transfer,
    keys::DeterministicTransferKeys,
    types::network::Network,
};

// `intent: PublicTransferIntent`, `keys: DeterministicTransferKeys` (account secret + pinned
// auth/seal nonce secrets, all canonical 32-byte Ristretto scalars).
let out = build_and_encode_public_transfer(Network::Esmeralda, &intent, &keys)?;
// out.encoded_transaction — submit-ready BOR bytes (lowercase hex at the boundary)
// out.transaction_id      — the 32-byte transaction id (chains the signatures)
```

This path is locked two ways:

- **Golden-vector-locked.** Committed fixtures under `fixtures/public_transfer/` pin the exact
  `encoded_transaction` + `transaction_id` for fixed intents/keys/nonces. The runner
  (`tests/golden_vectors.rs::run_golden_vectors`) reproduces them **byte-for-byte**; `expected` is
  generator-owned (never hand-edited). One vector (`large_amount`) uses an amount `> 2^53` µTari to
  prove the u64-safe path carries it intact end-to-end.
- **Orchestration-parity cross-checked vs the shared `ootle-rs` builder path.**
  `tests/ootle_rs_crosscheck.rs` re-derives `ootle-rs`'s `IAccount::public_transfer` recipe directly
  on the shared `tari_ootle_transaction::TransactionBuilder` (same pinned nonces) and asserts it
  encodes the identical bytes + id.

  > **Honest framing (ledger R4):** `ootle-rs` reuses the **same** `TransactionBuilder`, seal, and
  > `tari_bor` encoder this core uses — it has no independent encoder. So this cross-check proves the
  > core's **orchestration matches production** (instruction sequence, fee placement, input ordering,
  > bucket-label convention), **not** that two independent CBOR encoders agree. The truly-independent
  > encoder drift check is the TS/Python re-ports in Phase 4.

## What works today (Phase 2 — closed)

- **Input resolution** — two-phase want-list / fetched-substate resolution is landed (`src/inputs.rs`:
  `build_public_transfer_unsigned_with_wants` + `apply_fetched_substates`), so the public-transfer
  path no longer needs an explicit pre-resolved input set.
- **Result parsing** — `src/result.rs` (`parse_finalized_result` / `finalized_from_execute_result`)
  parses a finalized engine result into the boundary `FinalizedResult` (outcome, fee receipt, diff
  summary, events, logs, reject reasons) — beyond just submit-ack typing.
- **Binding generators (Go)** — the `ootle_sdk_ffi_c` flat `extern "C"` facade + cbindgen header and
  the greenfield `ootle-go` SDK are landed (Decision D4 — Go first).

## Not yet done

- **Stealth / confidential transfers** — `src/stealth.rs` is the one real remaining gap. Phase 3 adds
  the pure/sync send (intent + pinned `StealthEntropy` + fetched/decrypted inputs → bytes) and receive
  (inbound UTXO + view key → recovered value/mask/memo) surface; step 01 lands the boundary types and
  the `StealthEntropy` injection contract.
- **Kotlin/Swift/WASM facades** — only the Go facade exists today; UniFFI / WASM hosts come later.
- **Independent-encoder drift check** — lands with the TS/Python re-ports (Phase 4).

Next batch: **Phase 3 — confidential (stealth) transfers** over the fat core + Go SDK, per the
roadmap (`temp/001-fat-core-start/brainstorming/05-roadmap.md`, Phase 3).
