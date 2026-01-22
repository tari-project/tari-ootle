//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::dsl;
use tari_ootle_wallet_sdk::{
    models::{StealthOutputInfo, StealthOutputModel},
    storage::WalletStorageError,
};
use tari_template_lib_types::{
    crypto::{RistrettoPublicKeyBytes, UtxoTag},
    ComponentAddress,
    EncryptedData,
};
use time::PrimitiveDateTime;

use crate::{
    schema::stealth_outputs,
    serialization::{deserialize_hex_try_from, deserialize_json},
};

#[derive(Debug, Clone, Identifiable, Queryable)]
#[diesel(table_name = stealth_outputs)]
#[allow(clippy::struct_excessive_bools)]
pub struct StealthOutput {
    pub id: i32,
    pub owner_account_id: i32,
    pub resource_address: String,
    pub commitment: String,
    pub value: i64,
    pub sender_public_nonce: String,
    pub status: String,
    pub locked_at: Option<PrimitiveDateTime>,
    pub locked_by_proof: Option<i32>,
    pub view_only_key_id: String,
    pub owner_key_id: Option<String>,
    pub encrypted_data: Vec<u8>,
    pub tag_byte: i32,
    pub memo_json: Option<String>,
    pub spend_condition: String,
    pub minimum_value_promise: i64,
    pub is_burnt: bool,
    pub is_frozen: bool,
    pub is_on_chain: bool,
    pub is_condition_spendable: bool,
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
            value: self.value as u64,
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
            view_only_key_id: deserialize_json(&self.view_only_key_id)?,
            owner_key_id: self.owner_key_id.as_ref().map(deserialize_json).transpose()?,
            encrypted_data: EncryptedData::try_from(self.encrypted_data).map_err(|len| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output",
                    item: "output",
                    details: format!("Corrupt db: invalid encrypted data length {len}"),
                }
            })?,
            tag_byte: UtxoTag::new(self.tag_byte as u32),
            memo: self.memo_json.as_ref().map(deserialize_json).transpose()?,
            spend_condition: deserialize_json(&self.spend_condition)?,
            minimum_value_promise: self.minimum_value_promise as u64,
            status: self.status.parse().map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output",
                item: "output",
                details: format!("Corrupt db: invalid output status '{}'", self.status),
            })?,
            is_burnt: self.is_burnt,
            is_frozen: self.is_frozen,
            is_on_chain: self.is_on_chain,
            is_condition_spendable: self.is_condition_spendable,
            lock_id: self.locked_by_proof,
        })
    }
}

impl TryFrom<StealthOutput> for StealthOutputInfo {
    type Error = WalletStorageError;

    fn try_from(value: StealthOutput) -> Result<Self, Self::Error> {
        Ok(StealthOutputInfo {
            resource_address: value
                .resource_address
                .parse()
                .map_err(|_| WalletStorageError::DecodingError {
                    operation: "try_into_output_info",
                    item: "output info",
                    details: format!("Corrupt db: invalid resource address '{}'", value.resource_address),
                })?,
            public_nonce: RistrettoPublicKeyBytes::from_hex(&value.sender_public_nonce).map_err(|_| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output_info",
                    item: "output info public nonce",
                    details: "Corrupt db: invalid hex representation".to_string(),
                }
            })?,
            encrypted_data: EncryptedData::try_from(value.encrypted_data).map_err(|len| {
                WalletStorageError::DecodingError {
                    operation: "try_into_output_info",
                    item: "output info encrypted data",
                    details: format!("Corrupt db: invalid encrypted data length {len}"),
                }
            })?,
            commitment: deserialize_hex_try_from(&value.commitment).map_err(|_| WalletStorageError::DecodingError {
                operation: "try_into_output_info",
                item: "output info commitment",
                details: "Corrupt db: invalid hex representation".to_string(),
            })?,
            value: value.value as u64,
            memo: value.memo_json.as_ref().map(deserialize_json).transpose()?,
            is_on_chain: value.is_on_chain,
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
