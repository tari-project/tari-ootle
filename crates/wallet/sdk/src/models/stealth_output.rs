//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::{
    models::{ComponentAddress, EncryptedData},
    prelude::{PedersenCommitmentBytes, ResourceAddress, RistrettoPublicKeyBytes},
    types::Amount,
};

use crate::models::{OutputLockId, OutputStatus};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StealthOutputModel {
    pub owner_account: ComponentAddress,
    pub resource_address: ResourceAddress,
    pub commitment: PedersenCommitmentBytes,
    pub value: Amount,
    pub sender_public_nonce: RistrettoPublicKeyBytes,
    pub encryption_secret_key_index: u64,
    pub encrypted_data: EncryptedData,
    pub status: OutputStatus,
    pub lock_id: Option<OutputLockId>,
}

pub struct StealthBalance {
    pub balance: Amount,
    pub utxo_count: usize,
}
