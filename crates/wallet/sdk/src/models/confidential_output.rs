//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use tari_template_lib::{
    models::{EncryptedData, VaultId},
    prelude::{ComponentAddress, PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    types::Amount,
};

use crate::models::WalletLockId;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConfidentialOutputModel {
    pub account_address: ComponentAddress,
    pub vault_id: VaultId,
    pub commitment: PedersenCommitmentBytes,
    pub value: Amount,
    pub sender_public_nonce: Option<RistrettoPublicKeyBytes>,
    pub encryption_secret_key_index: u64,
    pub encrypted_data: EncryptedData,
    pub public_asset_tag: Option<RistrettoPublicKeyBytes>,
    pub status: OutputStatus,
    pub lock_id: Option<WalletLockId>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum OutputStatus {
    /// The output is available for spending
    Unspent,
    /// The output has been spent.
    Spent,
    /// The output is locked for spending. Once the transaction has been accepted, this output becomes Spent.
    LockedForSpend,
    /// The output is locked as an unconfirmed output. Once the transaction has been accepted, this output becomes
    /// Unspent.
    LockedUnconfirmed,
    /// This output existing in the vault but could not be validated successfully, meaning the encrypted value and/or
    /// mask were not constructed correctly by the sender. This output will not "be counted" in the confidential
    /// balance.
    Invalid,
}

impl OutputStatus {
    pub fn as_key_str(&self) -> &'static str {
        match self {
            Self::Unspent => "Unspent",
            Self::Spent => "Spent",
            Self::LockedForSpend => "LockedForSpend",
            Self::LockedUnconfirmed => "LockedUnconfirmed",
            Self::Invalid => "Invalid",
        }
    }
}

impl FromStr for OutputStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Unspent" => Ok(Self::Unspent),
            "Spent" => Ok(Self::Spent),
            "LockedForSpend" => Ok(Self::LockedForSpend),
            "LockedUnconfirmed" => Ok(Self::LockedUnconfirmed),
            "Invalid" => Ok(Self::Invalid),
            _ => Err(()),
        }
    }
}
