//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSecretKey};
use tari_engine_types::crypto::commit_u64_amount;
use tari_template_lib::{
    models::SpendCondition,
    types::{crypto::UtxoTag, EncryptedData},
};

use crate::memo::Memo;

#[derive(Debug, Clone)]
pub struct OutputWitness {
    pub amount: u64,
    pub mask: RistrettoSecretKey,
    pub sender_public_nonce: RistrettoPublicKey,
    pub minimum_value_promise: u64,
    pub encrypted_data: EncryptedData,
    pub resource_view_key: Option<RistrettoPublicKey>,
}

impl OutputWitness {
    pub fn to_commitment(&self) -> PedersenCommitment {
        commit_u64_amount(&self.mask, self.amount)
    }
}

#[derive(Debug, Clone)]
pub struct SecretStealthOutputStatement {
    pub witness: OutputWitness,
    pub spend_condition: SpendCondition,
    pub tag: UtxoTag,
}

#[derive(Debug, Clone)]
pub struct MaskAndValue {
    pub value: u64,
    pub mask: RistrettoSecretKey,
}

impl MaskAndValue {
    pub fn new(value: u64, mask: RistrettoSecretKey) -> Self {
        Self { value, mask }
    }

    pub fn to_commitment(&self) -> PedersenCommitment {
        commit_u64_amount(&self.mask, self.value)
    }
}

#[derive(Debug, Clone)]
pub struct DecryptedData {
    pub mask_and_value: MaskAndValue,
    pub memo: Option<Memo>,
}

impl DecryptedData {
    pub fn into_mask_and_value(self) -> MaskAndValue {
        self.mask_and_value
    }

    pub fn value(&self) -> u64 {
        self.mask_and_value.value
    }

    pub fn mask(&self) -> &RistrettoSecretKey {
        &self.mask_and_value.mask
    }

    pub fn memo(&self) -> Option<&Memo> {
        self.memo.as_ref()
    }

    pub fn to_commitment(&self) -> PedersenCommitment {
        self.mask_and_value.to_commitment()
    }
}

#[derive(Debug, Clone)]
pub struct StealthInputWitness {
    pub mask_and_value: MaskAndValue,
    pub public_nonce: RistrettoPublicKey,
}
