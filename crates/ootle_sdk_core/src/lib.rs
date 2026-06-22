//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#![forbid(unsafe_code)]
//! `ootle_sdk_core` — synchronous, pure transaction-construction primitives for the Ootle network.
//!
//! Every entry point is a pure, synchronous function: it maps a developer intent to submit-ready
//! BOR-encoded transaction bytes (and decodes results on the way back). It performs no I/O — no
//! sockets, async runtime, or clock — so all randomness and key material are supplied by the caller.

pub mod address_codec;
pub mod builder;
pub mod cosign;
pub mod faucet;
pub mod generic_builder;
pub mod identity;
pub mod inputs;
pub mod keys;
pub mod public_transfer;
pub mod resolved_transfer;
pub mod result;
pub mod seed;
pub mod stealth;
pub mod substate_decode;
pub mod tx;
pub mod types;

pub use address_codec::{ParsedAddress, format_identity_address, parse_address};
pub use cosign::{
    Authorization,
    UnsignedTransactionRecord,
    add_signature,
    add_signature_with_seed,
    seal_and_encode_with_auth,
    seal_and_encode_with_auth_with_seed,
    unsigned_record_for_cosign,
};
pub use faucet::{FaucetClaimIntent, build_faucet_claim_with_wants};
pub use generic_builder::{build_unsigned_instructions_with_wants, resolve_and_encode_instructions_with_seed};
pub use identity::{
    OotleKeypair,
    derive_account_address,
    derive_account_keypair_from_seed,
    derive_view_keypair_from_seed,
    generate_account_keypair,
    generate_view_keypair,
};
pub use inputs::{
    FetchedSubstate,
    PartialTransaction,
    Resolution,
    StealthSignerEntry,
    WantItem,
    WantList,
    apply_fetched_substates,
    apply_fetched_substates_with_secrets,
    build_public_transfer_unsigned_with_wants,
};
pub use public_transfer::{
    EncodedPublicTransfer,
    build_and_encode_public_transfer,
    build_and_encode_public_transfer_with_seed,
};
pub use resolved_transfer::{
    resolve_and_encode_public_transfer_with_seed,
    seal_and_encode_public_transfer,
    seal_and_encode_public_transfer_with_seed,
};
pub use result::{finalized_from_execute_result, parse_dry_run_result, parse_finalized_result};
pub use seed::{derive_cosign_nonce, derive_transfer_nonces};
pub use stealth::{
    StealthBuildCtx,
    StealthKeys,
    StealthPartialTransaction,
    StealthResolution,
    StealthSignatureRequirementsState,
    apply_fetched_substates_stealth,
    build_and_encode_stealth_transfer,
    build_and_encode_stealth_transfer_with_seed,
    build_stealth_transfer_unsigned,
    build_stealth_transfer_unsigned_with_seed,
    build_stealth_unsigned_with_wants,
    build_stealth_unsigned_with_wants_with_seed,
    decode_and_canonicalize_sealed_transfer,
    decode_stealth_utxo,
    scan_stealth_output,
    scan_stealth_substate,
    seal_and_encode_stealth_transfer,
    seal_and_encode_stealth_transfer_with_seed,
};
pub use substate_decode::{
    DecodedSubstate,
    ResourceBalance,
    VaultKind,
    account_balance_wants,
    account_balances,
    decode_substate,
};
pub use types::generic_intent::{
    ArgValue,
    BlobSpec,
    ComponentRef,
    FeeSource,
    GenericTransactionIntent,
    InstructionSpec,
    OwnerRuleSpec,
    encode_arg,
    workspace_arg,
};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {}
}
