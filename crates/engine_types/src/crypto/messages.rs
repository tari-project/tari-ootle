//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{RistrettoPublicKey, pedersen::PedersenCommitment};
use tari_template_lib::types::{
    Amount,
    Hash32,
    crypto::PedersenCommitmentBytes,
    stealth::{StealthInputsStatement, StealthOutputsStatement, ViewableBalanceProofMessageFields},
};

use crate::{
    Hash64,
    hashing::{EngineHashDomainLabel, engine_hasher64},
};

pub fn confidential_withdraw64(
    excess: &RistrettoPublicKey,
    public_nonce: &RistrettoPublicKey,
    input_revealed_amount: &Amount,
    output_revealed_amount: &Amount,
) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::ConfidentialTransfer)
        .chain(excess)
        .chain(public_nonce)
        .chain(input_revealed_amount)
        .chain(output_revealed_amount)
        .result()
        .into()
}

pub fn viewable_balance_proof64(
    commitment: &PedersenCommitment,
    view_key: &RistrettoPublicKey,
    challenge_fields: ViewableBalanceProofMessageFields<'_>,
) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::ViewableBalanceProof)
        .chain(commitment)
        .chain(view_key)
        .chain(&challenge_fields)
        .result()
        .into()
}

pub fn stealth_balance_proof64(
    public_excess: &RistrettoPublicKey,
    public_nonce: &RistrettoPublicKey,
    stealth_inputs_statement: &StealthInputsStatement,
    stealth_outputs_statement: &StealthOutputsStatement,
) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::StealthBalanceProof)
        .chain(public_excess)
        .chain(public_nonce)
        .chain(stealth_inputs_statement)
        .chain(stealth_outputs_statement)
        .result()
        .into()
}

/// Challenge for a covenant sub-balance proof (TIP-0006 `AssertCovenantBalanced`). Bound to a distinct domain from
/// [`stealth_balance_proof64`] so a partition proof can never be replayed as, or mistaken for, the whole-transfer
/// balance proof. The `condition_root`, `revealed_amount` and the ordered commitment lists pin the proof to one
/// partition.
pub fn covenant_balance_proof64(
    public_excess: &RistrettoPublicKey,
    public_nonce: &RistrettoPublicKey,
    condition_root: &Hash32,
    revealed_amount: &Amount,
    input_commitments: &[PedersenCommitmentBytes],
    output_commitments: &[PedersenCommitmentBytes],
) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::CovenantBalanceProof)
        .chain(public_excess)
        .chain(public_nonce)
        .chain(condition_root)
        .chain(revealed_amount)
        .chain(input_commitments)
        .chain(output_commitments)
        .result()
        .into()
}

pub fn value_proof_message(commitment: &PedersenCommitmentBytes, value: &Amount) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::ValueProof)
        .chain(commitment)
        .chain(value)
        .result()
        .into()
}
