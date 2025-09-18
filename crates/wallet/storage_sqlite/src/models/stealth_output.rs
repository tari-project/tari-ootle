//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::dsl;
use tari_ootle_wallet_sdk::{models::StealthOutputModel, storage::WalletStorageError};
use tari_template_lib::{
    models::{ComponentAddress, EncryptedData},
    types::{
        amount,
        crypto::{RistrettoPublicKeyBytes, UtxoTag},
    },
};
use time::PrimitiveDateTime;

use crate::{schema::stealth_outputs, serialization::deserialize_hex_try_from};

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
    pub tag_byte: i32,
    pub is_burnt: bool,
    pub is_frozen: bool,
    pub is_on_chain: bool,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl StealthOutput {
    pub(crate) fn try_convert(self, owner_account: ComponentAddress) -> Result<StealthOutputModel, WalletStorageError> {
        Ok(StealthOutputModel {
            owner_account,
            resource_address: self
                .resource_address
                .parse()
                .map_err(|_| WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid resource address '{}'", self.resource_address),
                })?,
            commitment: deserialize_hex_try_from(&self.commitment).map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output",
                item: "output commitment",
                details: "Corrupt db: invalid hex representation".to_string(),
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
            tag_byte: UtxoTag::new(self.tag_byte as u32),
            status: self.status.parse().map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output",
                item: "output",
                details: format!("Corrupt db: invalid output status '{}'", self.status),
            })?,
            is_burnt: self.is_burnt,
            is_frozen: self.is_frozen,
            is_on_chain: self.is_on_chain,
            lock_id: self.locked_by_proof,
        })
    }
}

#[derive(AsChangeset)]
#[diesel(table_name = stealth_outputs)]
pub(crate) struct StealthOutputUpdate<'a> {
    pub status: Option<&'a str>,
    pub is_burnt: Option<bool>,
    pub is_frozen: Option<bool>,
    pub updated_at: dsl::now,
}
