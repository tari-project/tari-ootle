//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::{Identifiable, Queryable};
use tari_ootle_wallet_sdk::storage::WalletStorageError;
use time::PrimitiveDateTime;

use crate::schema::accounts;

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = accounts)]
pub struct Account {
    pub id: i32,
    pub name: Option<String>,
    pub address: String,
    pub owner_key_index: i64,
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
            address: self.address.parse().map_err(|e| WalletStorageError::DecodingError {
                operation: "Account::try_convert",
                item: "address",
                details: format!("Invalid address: {}: {e}", self.address),
            })?,
            key_index: self.owner_key_index as u64,
            is_confirmed_on_chain: self.is_confirmed_on_chain,
            is_default: self.is_default,
        })
    }
}
