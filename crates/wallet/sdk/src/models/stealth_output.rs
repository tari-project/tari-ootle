//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_crypto::memo::Memo;
use tari_template_lib::{
    models::{ComponentAddress, SpendCondition, UtxoAddress},
    prelude::{PedersenCommitmentBytes, ResourceAddress, RistrettoPublicKeyBytes},
    types::{crypto::UtxoTag, Amount, EncryptedData},
};

use crate::models::{KeyId, OutputStatus, WalletLockId};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct StealthOutputModel {
    pub owner_account: ComponentAddress,
    pub resource_address: ResourceAddress,
    pub commitment: PedersenCommitmentBytes,
    pub value: u64,
    pub sender_public_nonce: RistrettoPublicKeyBytes,
    /// Note: this field is more for debugging. We use the account key index for all outputs belonging to an account
    pub view_only_key_id: KeyId,
    /// None means this output cannot be spent, it's view-only
    pub owner_key_id: Option<KeyId>,
    pub encrypted_data: EncryptedData,
    pub tag_byte: UtxoTag,
    pub memo: Option<Memo>,
    pub spend_condition: SpendCondition,
    pub minimum_value_promise: u64,
    pub status: OutputStatus,
    pub is_burnt: bool,
    pub is_frozen: bool,
    pub is_on_chain: bool,
    pub is_condition_spendable: bool,
    pub lock_id: Option<WalletLockId>,
}

impl StealthOutputModel {
    pub fn to_utxo_address(&self) -> UtxoAddress {
        UtxoAddress::new(self.resource_address, self.commitment.into())
    }

    pub fn into_spend_data(self) -> InputSpendData {
        InputSpendData {
            commitment: self.commitment,
            public_nonce: self.sender_public_nonce,
            encrypted_data: self.encrypted_data,
            value: self.value,
            is_on_chain: self.is_on_chain,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StealthOutputInfo {
    pub resource_address: ResourceAddress,
    pub commitment: PedersenCommitmentBytes,
    pub public_nonce: RistrettoPublicKeyBytes,
    pub encrypted_data: EncryptedData,
    pub value: u64,
    pub memo: Option<Memo>,
    pub is_on_chain: bool,
}

#[derive(Debug, Clone)]
pub struct InputSpendData {
    pub commitment: PedersenCommitmentBytes,
    pub public_nonce: RistrettoPublicKeyBytes,
    pub encrypted_data: EncryptedData,
    pub value: u64,
    pub is_on_chain: bool,
}

impl From<StealthOutputInfo> for InputSpendData {
    fn from(info: StealthOutputInfo) -> Self {
        Self {
            commitment: info.commitment,
            public_nonce: info.public_nonce,
            encrypted_data: info.encrypted_data,
            value: info.value,
            is_on_chain: info.is_on_chain,
        }
    }
}

pub struct StealthBalance {
    pub balance: Amount,
    pub utxo_count: usize,
}
