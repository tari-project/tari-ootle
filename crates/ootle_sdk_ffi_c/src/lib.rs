//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! `ootle_sdk_ffi_c` — the flat **C ABI facade** over [`ootle_sdk_core`].
//!
//! This crate is **mechanical and thin**: it marshals JSON ↔ the core's serde boundary records,
//! threads the one opaque [`PartialTransaction`](ootle_sdk_core::PartialTransaction) handle, and
//! converts every `Result`/panic into a stable [`OotleResult`] envelope. **No domain logic lives
//! here** — all value-critical work stays in the core.
//!
//! ## The calling convention (one shape for every op)
//!
//! - Scalars cross as C primitives (`uint8_t network`).
//! - Structured arguments cross as `const char*` UTF-8 JSON — exactly the serde form of the core's boundary records
//!   ([`PublicTransferIntent`], [`FetchedSubstate`] arrays, the raw indexer result string, and a facade-local key
//!   mirror).
//! - Every op returns an [`OotleResult`] by value: `ok` flag, `error_code`/`error_message` (heap-owned C strings, empty
//!   on success), and the output as either `data_json` (heap-owned JSON C string) **or** a `handle` (the opaque
//!   [`PartialTransaction`] pointer).
//!
//! ## Memory & ownership (host owns and frees everything)
//!
//! - The host **must** call [`ootle_result_free`] on every returned [`OotleResult`] exactly once. That frees the
//!   envelope's three C strings (`error_code`, `error_message`, `data_json`). It does **not** touch `handle`.
//! - A returned `handle` is owned by the host and must be freed exactly once with [`ootle_partial_transaction_free`] —
//!   **unless** it was consumed by a call that takes a handle (see below).
//! - There is exactly **one** alloc/free pair per buffer. Both free functions are null-safe.
//!
//! ## The opaque handle lifecycle (the subtle part — read carefully)
//!
//! [`PartialTransaction`](ootle_sdk_core::PartialTransaction) is the one opaque handle
//! (`OotlePartialTransaction`). Its lifecycle:
//!
//! 1. [`ootle_build_unsigned`] returns a fresh handle (in `OotleResult.handle`) plus the want list in `data_json`.
//! 2. [`ootle_apply_fetched_substates`] **consumes** the handle passed in — the caller must treat the pointer it passed
//!    as invalid afterwards and use **only** the handle in the returned envelope. On success the returned envelope
//!    carries a (possibly different) handle to thread forward, and `data_json` carries the resolution status
//!    (`{"status":"resolved"}` or `{"status":"need_more","want_list":[…],"fetch_ids":[…]}`, where `fetch_ids` is the
//!    authoritative next-fetch set). **On a processing error (parse/resolution) the non-null input handle is still
//!    consumed and freed** — do not free it again. (Passing a *null* handle is a precondition violation that yields an
//!    `"INVALID"` envelope and consumes nothing.)
//! 3. [`ootle_seal_and_encode`] **consumes** the handle and returns the encoded transaction in `data_json`; no handle
//!    comes back. The host must **not** free a consumed handle.
//!
//! Because a consumed handle is always taken by value (Rust `Box::from_raw`), the host frees a handle
//! with [`ootle_partial_transaction_free`] **only** when it never reaches a consuming call (e.g. it
//! aborts the flow after `build_unsigned`). Passing the same handle to two consuming calls, or freeing
//! a consumed handle, is a use-after-free — the classic FFI bug this contract is designed to prevent.
//!
//! ## No panics cross the boundary
//!
//! Every `extern "C"` body is wrapped in [`std::panic::catch_unwind`]; a panic becomes an
//! `"INTERNAL"` error in the envelope, never undefined behaviour.

mod c_abi;
mod stealth_abi;
mod substate_decode_abi;

pub use c_abi::{
    OotlePartialTransaction,
    OotleResult,
    ootle_abi_version,
    ootle_add_signature,
    ootle_apply_fetched_substates,
    ootle_build_and_encode_public_transfer,
    ootle_build_and_encode_public_transfer_production,
    ootle_build_faucet_claim,
    ootle_build_unsigned,
    ootle_build_unsigned_instructions,
    ootle_derive_account_address,
    ootle_derive_account_key_from_seed,
    ootle_derive_view_key_from_seed,
    ootle_format_identity_address,
    ootle_generate_account_key,
    ootle_generate_view_key,
    ootle_parse_address,
    ootle_parse_finalized_result,
    ootle_partial_transaction_free,
    ootle_result_free,
    ootle_seal_and_encode,
    ootle_seal_and_encode_production,
    ootle_seal_and_encode_with_auth,
    ootle_seal_and_encode_with_auth_production,
    ootle_string_free,
    ootle_unsigned_record_for_cosign,
};
pub use stealth_abi::{
    OotleStealthPartialTransaction,
    ootle_apply_fetched_substates_stealth,
    ootle_build_and_encode_stealth_transfer,
    ootle_build_and_encode_stealth_transfer_production,
    ootle_build_stealth_outputs_statement,
    ootle_build_stealth_unsigned,
    ootle_build_stealth_unsigned_production,
    ootle_decode_stealth_utxo,
    ootle_scan_stealth_output,
    ootle_scan_stealth_substate,
    ootle_seal_and_encode_stealth,
    ootle_seal_and_encode_stealth_production,
    ootle_stealth_partial_transaction_free,
    ootle_validate_stealth_transfer,
};
pub use substate_decode_abi::{ootle_account_balance_wants, ootle_account_balances, ootle_decode_substate};
