//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::{models::ConfidentialOutputModel, storage::WalletStorageError};
use tari_template_lib::{
    models::EncryptedData,
    types::crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
};
use time::PrimitiveDateTime;

use crate::schema::confidential_outputs;

#[derive(Debug, Clone, Identifiable, Queryable)]
#[diesel(table_name = confidential_outputs)]
pub struct ConfidentialOutput {
    pub id: i32,
    pub account_id: i32,
    pub vault_id: i32,
    pub commitment: String,
    // TODO: change to a string to allow arbitrary precision
    pub value: i64,
    pub sender_public_nonce: Option<String>,
    pub encryption_secret_key_index: i64,
    pub public_asset_tag: Option<String>,
    pub status: String,
    pub locked_at: Option<PrimitiveDateTime>,
    pub locked_by_proof: Option<i32>,
    pub encrypted_data: Vec<u8>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl ConfidentialOutput {
    pub(crate) fn try_into_output(
        self,
        account_address_str: &str,
        vault_id: &str,
    ) -> Result<ConfidentialOutputModel, WalletStorageError> {
        Ok(ConfidentialOutputModel {
            account_address: account_address_str
                .parse()
                .map_err(|_| WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid account address '{}'", account_address_str),
                })?,
            vault_id: vault_id.parse().map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output",
                item: "output",
                details: format!("Corrupt db: invalid vault address '{}'", self.vault_id),
            })?,
            commitment: PedersenCommitmentBytes::from_hex(&self.commitment).map_err(|_| {
                WalletStorageError::DecodingError {
                    operation: "outputs_lock_smallest_amount",
                    item: "output commitment",
                    details: "Corrupt db: invalid hex representation".to_string(),
                }
            })?,
            value: (self.value as u64).into(),
            sender_public_nonce: self
                .sender_public_nonce
                .map(|nonce| RistrettoPublicKeyBytes::from_hex(&nonce).unwrap()),
            encryption_secret_key_index: self.encryption_secret_key_index as u64,
            encrypted_data: EncryptedData::try_from(self.encrypted_data).map_err(|len| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid encrypted data length {len}"),
                }
            })?,
            public_asset_tag: self
                .public_asset_tag
                .map(|tag| RistrettoPublicKeyBytes::from_hex(&tag).unwrap()),
            status: self.status.parse().map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output",
                item: "output",
                details: format!("Corrupt db: invalid output status '{}'", self.status),
            })?,
            lock_id: self.locked_by_proof,
        })
    }
}
