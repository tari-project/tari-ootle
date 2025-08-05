//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey};
use tari_template_lib::{
    models::ViewableBalanceProofChallengeFields,
    prelude::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    types::Amount,
};

use crate::hashing::{hasher64, EngineHashDomainLabel};

pub fn confidential_withdraw64(
    excess: &RistrettoPublicKey,
    public_nonce: &RistrettoPublicKey,
    input_revealed_amount: &Amount,
    output_revealed_amount: &Amount,
) -> [u8; 64] {
    hasher64(EngineHashDomainLabel::ConfidentialTransfer)
        .chain(excess)
        .chain(public_nonce)
        .chain(input_revealed_amount)
        .chain(output_revealed_amount)
        .result()
}

pub fn viewable_balance_proof64(
    commitment: &PedersenCommitment,
    view_key: &RistrettoPublicKey,
    challenge_fields: ViewableBalanceProofChallengeFields<'_>,
) -> [u8; 64] {
    hasher64(EngineHashDomainLabel::ViewableBalanceProof)
        .chain(commitment)
        .chain(view_key)
        .chain(&challenge_fields)
        .result()
}

pub fn stealth_mint64(
    public_excess: &RistrettoPublicKey,
    public_nonce: &RistrettoPublicKey,
    total_amount: Amount,
) -> [u8; 64] {
    hasher64(EngineHashDomainLabel::StealthMint)
        .chain(public_excess)
        .chain(public_nonce)
        .chain(&total_amount)
        .result()
}

pub fn stealth_transfer64(
    public_excess: &RistrettoPublicKey,
    public_nonce: &RistrettoPublicKey,
    input_revealed_amount: &Amount,
    output_revealed_amount: &Amount,
) -> [u8; 64] {
    hasher64(EngineHashDomainLabel::StealthTransfer)
        .chain(public_excess)
        .chain(public_nonce)
        .chain(input_revealed_amount)
        .chain(output_revealed_amount)
        .result()
}

pub fn stealth_ownership64(
    public_key: &RistrettoPublicKeyBytes,
    public_nonce: &RistrettoPublicKeyBytes,
    commitment: &PedersenCommitmentBytes,
    public_output_nonce: &RistrettoPublicKeyBytes,
) -> [u8; 64] {
    hasher64(EngineHashDomainLabel::StealthOwnership)
        .chain(public_key)
        .chain(public_nonce)
        .chain(commitment)
        .chain(public_output_nonce)
        .result()
}
