//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey, pedersen::PedersenCommitment};
use tari_engine_types::crypto::commit_u64_amount;
use tari_template_lib_types::{EncryptedData, crypto::UtxoTag, stealth::SpendCondition};

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
pub struct StealthOutputWitness {
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
    /// The spend condition of the UTXO being spent. Required to partition inputs by covenant when generating
    /// covenant balance proofs (TIP-0006); `None` for inputs that do not participate in a covenant.
    pub spend_condition: Option<SpendCondition>,
}

impl StealthInputWitness {
    pub fn new(mask_and_value: MaskAndValue) -> Self {
        Self {
            mask_and_value,
            spend_condition: None,
        }
    }

    pub fn with_spend_condition(mask_and_value: MaskAndValue, spend_condition: SpendCondition) -> Self {
        Self {
            mask_and_value,
            spend_condition: Some(spend_condition),
        }
    }
}
