//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{pedersen::PedersenCommitment, RistrettoPublicKey, RistrettoSecretKey};
use tari_engine_types::crypto::commit_amount_checked;
use tari_template_lib::{models::EncryptedData, types::Amount};

#[derive(Debug, Clone)]
pub struct ConfidentialProofStatement {
    pub amount: Amount,
    pub mask: RistrettoSecretKey,
    pub sender_public_nonce: RistrettoPublicKey,
    pub minimum_value_promise: u64,
    pub encrypted_data: EncryptedData,
    pub resource_view_key: Option<RistrettoPublicKey>,
}

impl ConfidentialProofStatement {
    pub fn to_commitment(&self) -> Option<PedersenCommitment> {
        commit_amount_checked(&self.mask, self.amount)
    }
}
