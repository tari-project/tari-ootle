//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use diesel::{Identifiable, Queryable};
use tari_ootle_wallet_sdk::storage::WalletStorageError;
use tari_template_lib::{models::ResourceAddress, prelude::ResourceType, types::Amount};
use time::PrimitiveDateTime;

use crate::{schema::resources, serialization::deserialize_json};

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = resources)]
pub struct Resource {
    pub id: i32,
    pub address: String,
    pub resource_type: String,
    pub token_symbol: Option<String>,
    pub divisibility: i32,
    pub metadata: String,
    pub access_rules: String,
    pub total_supply: Option<String>,
    pub updated_at: PrimitiveDateTime,
    pub created_at: PrimitiveDateTime,
}

impl Resource {
    pub(crate) fn try_convert(self) -> Result<tari_ootle_wallet_sdk::models::ResourceModel, WalletStorageError> {
        Ok(tari_ootle_wallet_sdk::models::ResourceModel {
            address: ResourceAddress::from_str(&self.address).map_err(|e| WalletStorageError::DecodingError {
                operation: "try_into_vault",
                item: "vault.resource_address",
                details: e.to_string(),
            })?,
            resource_type: ResourceType::from_str(&self.resource_type).map_err(|e| {
                WalletStorageError::DecodingError {
                    operation: "try_into_vault",
                    item: "vault.resource_type",
                    details: e.to_string(),
                }
            })?,
            token_symbol: self.token_symbol,
            divisibility: u8::try_from(self.divisibility as u32).map_err(|e| WalletStorageError::DecodingError {
                operation: "try_into_vault",
                item: "vault.divisibility",
                details: format!("Invalid divisibility: {}", e),
            })?,
            metadata: deserialize_json(&self.metadata).map_err(|e| WalletStorageError::DecodingError {
                operation: "try_into_vault",
                item: "vault.metadata",
                details: e.to_string(),
            })?,
            access_rules: deserialize_json(&self.access_rules).map_err(|e| WalletStorageError::DecodingError {
                operation: "try_into_vault",
                item: "vault.access_rules",
                details: e.to_string(),
            })?,

            total_supply: self
                .total_supply
                .map(|s| {
                    Amount::from_str(&s).map_err(|e| WalletStorageError::DecodingError {
                        operation: "try_into_vault",
                        item: "vault.total_supply",
                        details: e.to_string(),
                    })
                })
                .transpose()?,
        })
    }
}
