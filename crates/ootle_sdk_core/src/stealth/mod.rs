//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The confidential-transfer (**stealth**) surface.
//!
//! Two halves, both pure and synchronous:
//!
//! - **Send** — turn a [`StealthTransferIntent`] plus pinned [`StealthEntropy`] and fetched/decrypted inputs into
//!   submit-ready BOR bytes. The pipeline builds outputs, resolves + decrypts inputs, assembles the transfer statement,
//!   then signs/seals/encodes.
//! - **Receive** — recover the value/mask/memo of an inbound stealth UTXO from a view key (`scan_stealth_output`).
//!   Receive is pure and uses **no** RNG.
//!
//! This module re-exports the boundary records (defined in [`crate::types::stealth`]) at the crate's
//! stealth surface and declares the sub-modules that implement the send and receive halves.

pub use crate::types::stealth::{
    CommitmentBytes,
    DecryptedOutput,
    EncryptedDataBytes,
    InboundStealthOutput,
    PerOutputEntropy,
    RangeProofBytes,
    StealthEntropy,
    StealthInputSpec,
    StealthMemo,
    StealthOutputSpec,
    StealthPayTo,
    StealthTransferIntent,
    UtxoTagBytes,
};

/// Stealth **output** construction: injected commitment/AEAD/viewable-proof building + the aggregated
/// bulletproof.
pub mod outputs;

pub use outputs::{
    build_stealth_output_witness,
    build_stealth_outputs_statement,
    build_stealth_outputs_statement_with_seed,
};

/// Stealth **input** resolution: the UTXO fetch-want variant + the decrypt-in-core path that recovers
/// the spend mask from caller-supplied account secrets.
pub mod inputs;

pub use inputs::{spend_secrets_map, stealth_utxo_substate_id};

/// Stealth **statement assembly**: combine the outputs statement + inputs statement, generate +
/// locally validate the balance proof, run the `validate_transfer` pre-flight, and assemble
/// `Instruction::StealthTransfer` into an [`UnsignedTransaction`](tari_ootle_transaction).
pub mod assemble;

/// The opaque assembly output: the assembled unsigned tx + the internal signing requirements the seal
/// stage consumes.
pub mod partial;

pub use assemble::{
    StealthBuildCtx,
    StealthResolution,
    apply_fetched_substates_stealth,
    assemble_stealth_transfer_statement,
    build_stealth_transfer_unsigned,
    build_stealth_transfer_unsigned_with_seed,
    build_stealth_unsigned_with_wants,
    build_stealth_unsigned_with_wants_with_seed,
};
pub use partial::{StealthPartialTransaction, StealthSignatureRequirementsState};

/// Stealth **signing + sealing + encoding**: selects one of three seal keys
/// (account-key / stealth `c+k` / ephemeral) and produces submit-ready BOR bytes.
pub mod sign_seal;

pub use sign_seal::{
    StealthKeys,
    build_and_encode_stealth_transfer,
    build_and_encode_stealth_transfer_with_seed,
    seal_and_encode_stealth_transfer,
    seal_and_encode_stealth_transfer_with_seed,
};

/// Stealth **receive / scan**: the pure, RNG-free `scan_stealth_output` that decrypts an inbound
/// stealth UTXO with a view secret and decides whether it is addressed to the scanner.
/// DH → `unblind_output` → value/mask/memo + tag/ownership.
pub mod scan;

pub use scan::{scan_stealth_output, scan_stealth_substate};

/// Stealth **UTXO decode**: turn a fetched UTXO substate into the receive-shaped
/// [`InboundStealthOutput`] the scanner consumes; the field extraction is shared with the spend path's
/// input resolver.
pub mod decode;

pub use decode::decode_stealth_utxo;

/// Stealth **sealed-transfer canonicalization**: a pure decode + verify-all-signatures +
/// null-the-byte-unstable-fields helper for comparing sealed transfers semantically.
pub mod canonicalize;

pub use canonicalize::{UNSTABLE_NULL_SET, decode_and_canonicalize_sealed_transfer, null_unstable_fields};
