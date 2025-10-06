//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use diesel::{Identifiable, Queryable};
use tari_ootle_wallet_sdk::storage::WalletStorageError;
use tari_template_lib::{
    models::{ComponentAddress, ResourceAddress, VaultId},
    types::{Amount, ResourceType},
};
use time::PrimitiveDateTime;

use crate::schema::vaults;

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = vaults)]
pub struct Vault {
    pub id: i32,
    pub account_id: i32,
    pub address: String,
    pub resource_address: String,
    pub resource_type: String,
    pub revealed_balance: i64,
    pub confidential_balance: i64,
    pub locked_revealed_balance: i64,
    pub token_symbol: Option<String>,
    pub divisibility: i32,
    pub _locked_by: Option<i32>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl Vault {
    pub(crate) fn try_into_vault(
        self,
        account_address: ComponentAddress,
    ) -> Result<tari_ootle_wallet_sdk::models::VaultModel, WalletStorageError> {
        Ok(tari_ootle_wallet_sdk::models::VaultModel {
            account_address,
            id: VaultId::from_str(&self.address).map_err(|e| WalletStorageError::DecodingError {
                operation: "try_into_vault",
                item: "vault.address",
                details: e.to_string(),
            })?,
            resource_address: ResourceAddress::from_str(&self.resource_address).map_err(|e| {
                WalletStorageError::DecodingError {
                    operation: "try_into_vault",
                    item: "vault.resource_address",
                    details: e.to_string(),
                }
            })?,
            resource_type: ResourceType::from_str(&self.resource_type).map_err(|e| {
                WalletStorageError::DecodingError {
                    operation: "try_into_vault",
                    item: "vault.resource_type",
                    details: e.to_string(),
                }
            })?,
            token_symbol: self.token_symbol,
            revealed_balance: Amount::from(self.revealed_balance),
            locked_revealed_balance: Amount::from(self.locked_revealed_balance),
            confidential_balance: Amount::from(self.confidential_balance),
            divisibility: u8::try_from(self.divisibility as u32).map_err(|e| WalletStorageError::DecodingError {
                operation: "try_into_vault",
                item: "vault.divisibility",
                details: format!("Invalid divisibility: {}", e),
            })?,
        })
    }
}
