//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey};
use tari_template_lib::{
    models::{StealthInputsStatement, StealthOutputsStatement, ViewableBalanceProofChallengeFields},
    prelude::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    types::Amount,
};

use crate::{
    hashing::{engine_hasher64, EngineHashDomainLabel},
    Hash64,
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
    challenge_fields: ViewableBalanceProofChallengeFields<'_>,
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

pub fn stealth_ownership64(
    commitment: &PedersenCommitmentBytes,
    public_output_nonce: &RistrettoPublicKeyBytes,
    metadata_hash: &Hash64,
) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::StealthOwnership)
        .chain(commitment)
        .chain(public_output_nonce)
        .chain(metadata_hash)
        .result()
        .into()
}

pub fn stealth_statement_metadata64(outputs_statement: &StealthOutputsStatement) -> Hash64 {
    engine_hasher64(EngineHashDomainLabel::StealthOwnership)
        .chain(outputs_statement)
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
