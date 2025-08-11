//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::{models::StealthOutputModel, storage::WalletStorageError};
use tari_template_lib::{
    models::EncryptedData,
    types::{
        amount,
        crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    },
};
use time::PrimitiveDateTime;

use crate::schema::stealth_outputs;

#[derive(Debug, Clone, Identifiable, Queryable)]
#[diesel(table_name = stealth_outputs)]
pub struct StealthOutput {
    pub id: i32,
    pub owner_account_id: i32,
    pub resource_address: String,
    pub commitment: String,
    pub value: String,
    pub sender_public_nonce: String,
    pub status: String,
    pub locked_at: Option<PrimitiveDateTime>,
    pub locked_by_proof: Option<i32>,
    pub encryption_secret_key_index: i64,
    pub encrypted_data: Vec<u8>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl StealthOutput {
    pub(crate) fn try_convert(self, owner_account_str: &str) -> Result<StealthOutputModel, WalletStorageError> {
        Ok(StealthOutputModel {
            owner_account: owner_account_str
                .parse()
                .map_err(|e| WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid owner account address '{owner_account_str}': {e}"),
                })?,
            resource_address: self
                .resource_address
                .parse()
                .map_err(|_| WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid resource address '{}'", self.resource_address),
                })?,
            commitment: PedersenCommitmentBytes::from_hex(&self.commitment).map_err(|_| {
                WalletStorageError::DecodingError {
                    operation: "outputs_lock_smallest_amount",
                    item: "output commitment",
                    details: "Corrupt db: invalid hex representation".to_string(),
                }
            })?,
            value: amount![&self.value],
            sender_public_nonce: RistrettoPublicKeyBytes::from_hex(&self.sender_public_nonce).map_err(|_| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!(
                        "Corrupt db: invalid sender public nonce hex '{}'",
                        self.sender_public_nonce
                    ),
                }
            })?,
            encryption_secret_key_index: self.encryption_secret_key_index as u64,
            encrypted_data: EncryptedData::try_from(self.encrypted_data).map_err(|len| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid encrypted data length {len}"),
                }
            })?,
            status: self.status.parse().map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output",
                item: "output",
                details: format!("Corrupt db: invalid output status '{}'", self.status),
            })?,
            lock_id: self.locked_by_proof.map(|proof| proof as u64),
        })
    }
}
