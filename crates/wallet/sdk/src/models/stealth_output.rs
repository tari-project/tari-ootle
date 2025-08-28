//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::UtxoAddress;
use tari_template_lib::{
    models::{ComponentAddress, EncryptedData},
    prelude::{PedersenCommitmentBytes, ResourceAddress, RistrettoPublicKeyBytes},
    types::{crypto::UtxoTagByte, Amount},
};

use crate::models::{OutputStatus, WalletLockId};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StealthOutputModel {
    pub owner_account: ComponentAddress,
    pub resource_address: ResourceAddress,
    pub commitment: PedersenCommitmentBytes,
    pub value: Amount,
    pub sender_public_nonce: RistrettoPublicKeyBytes,
    pub encryption_secret_key_index: u64,
    pub encrypted_data: EncryptedData,
    pub tag_byte: UtxoTagByte,
    pub status: OutputStatus,
    pub is_burnt: bool,
    pub is_frozen: bool,
    pub is_on_chain: bool,
    pub lock_id: Option<WalletLockId>,
}

impl StealthOutputModel {
    pub fn to_utxo_address(&self) -> UtxoAddress {
        UtxoAddress::new(self.resource_address, self.commitment.into())
    }
}

pub struct StealthBalance {
    pub balance: Amount,
    pub utxo_count: usize,
}
