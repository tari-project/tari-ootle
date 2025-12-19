//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::{Identifiable, Queryable};
use tari_ootle_common_types::Epoch;
use tari_ootle_wallet_sdk::storage::WalletStorageError;
use time::PrimitiveDateTime;

use crate::{
    schema::accounts,
    serialization::{deserialize_hex_try_from, deserialize_json},
};

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = accounts)]
pub struct Account {
    pub id: i32,
    pub name: Option<String>,
    pub address: String,
    pub owner_public_key: String,
    pub view_only_key_id: String,
    pub owner_key_id: Option<String>,
    pub birthday_epoch: i64,
    pub is_default: bool,
    pub is_confirmed_on_chain: bool,
    pub _stealth_resource_address: String,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl Account {
    pub(crate) fn try_convert(self) -> Result<tari_ootle_wallet_sdk::models::Account, WalletStorageError> {
        Ok(tari_ootle_wallet_sdk::models::Account {
            name: self.name,
            component_address: self.address.parse().map_err(|e| WalletStorageError::DecodingError {
                operation: "Account::try_convert",
                item: "address",
                details: format!("Invalid address: {}: {e}", self.address),
            })?,
            owner_key_id: self.owner_key_id.as_ref().map(deserialize_json).transpose()?,
            view_only_key_id: deserialize_json(&self.view_only_key_id)?,
            owner_public_key: deserialize_hex_try_from(&self.owner_public_key)?,
            birthday_epoch: Epoch(self.birthday_epoch as u64),
            is_confirmed_on_chain: self.is_confirmed_on_chain,
            is_default: self.is_default,
        })
    }
}
