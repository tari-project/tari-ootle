//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSecretKey};
use tari_engine_types::crypto::commit_amount_checked;
use tari_template_lib::{
    models::EncryptedData,
    types::{crypto::UtxoTagByte, Amount},
};

#[derive(Debug, Clone)]
pub struct UnblindedOutputStatement {
    pub amount: Amount,
    pub mask: RistrettoSecretKey,
    pub sender_public_nonce: RistrettoPublicKey,
    pub minimum_value_promise: u64,
    pub encrypted_data: EncryptedData,
    pub resource_view_key: Option<RistrettoPublicKey>,
}

impl UnblindedOutputStatement {
    pub fn to_commitment(&self) -> Option<PedersenCommitment> {
        commit_amount_checked(&self.mask, self.amount)
    }
}

#[derive(Debug, Clone)]
pub struct UnblindedStealthOutputStatement {
    pub statement: UnblindedOutputStatement,
    pub output_owner_public_key: RistrettoPublicKey,
    pub tag: UtxoTagByte,
}

#[derive(Debug, Clone)]
pub struct MaskAndValue {
    pub value: Amount,
    pub mask: RistrettoSecretKey,
}

impl MaskAndValue {
    pub fn new(value: Amount, mask: RistrettoSecretKey) -> Self {
        Self { value, mask }
    }

    pub fn to_commitment(&self) -> Option<PedersenCommitment> {
        commit_amount_checked(&self.mask, self.value)
    }
}

#[derive(Debug, Clone)]
pub struct UnblindedStealthInputStatement {
    pub mask_and_value: MaskAndValue,
    pub owner_secret: RistrettoSecretKey,
    pub public_nonce: RistrettoPublicKey,
}
