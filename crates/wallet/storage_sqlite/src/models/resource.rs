//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use diesel::{Identifiable, Queryable};
use tari_engine_types::resource::Resource;
use tari_ootle_wallet_sdk::storage::WalletStorageError;
use tari_template_lib::{
    models::ResourceAddress,
    types::{Amount, ResourceType},
};
use time::PrimitiveDateTime;

use crate::{
    schema::resources,
    serialization::{deserialize_hex_try_from, deserialize_json},
};

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = resources)]
pub struct ResourceModel {
    pub id: i32,
    pub address: String,
    pub resource_type: String,
    pub owner_key: Option<String>,
    pub owner_rule: String,
    pub access_rules: String,
    pub token_symbol: Option<String>,
    pub divisibility: i32,
    pub metadata: String,
    pub total_supply: Option<String>,
    pub view_key: Option<String>,
    pub auth_hook: Option<String>,

    pub updated_at: PrimitiveDateTime,
    pub created_at: PrimitiveDateTime,
}

impl ResourceModel {
    pub(crate) fn try_convert(self) -> Result<tari_ootle_wallet_sdk::models::ResourceModel, WalletStorageError> {
        let address = ResourceAddress::from_str(&self.address).map_err(|e| WalletStorageError::DecodingError {
            operation: "try_convert",
            item: "resource.address",
            details: e.to_string(),
        })?;
        let resource_type =
            ResourceType::from_str(&self.resource_type).map_err(|e| WalletStorageError::DecodingError {
                operation: "try_convert",
                item: "resource.resource_type",
                details: e.to_string(),
            })?;
        let owner_key = self
            .owner_key
            .as_ref()
            .map(deserialize_hex_try_from)
            .transpose()
            .map_err(|e| WalletStorageError::DecodingError {
                operation: "try_convert",
                item: "resource.owner_key",
                details: e.to_string(),
            })?;
        let owner_rule = deserialize_json(&self.owner_rule).map_err(|e| WalletStorageError::DecodingError {
            operation: "try_convert",
            item: "resource.owner_rule",
            details: e.to_string(),
        })?;
        let access_rules = deserialize_json(&self.access_rules).map_err(|e| WalletStorageError::DecodingError {
            operation: "try_convert",
            item: "resource.access_rules",
            details: e.to_string(),
        })?;
        let divisibility = u8::try_from(self.divisibility as u32).map_err(|e| WalletStorageError::DecodingError {
            operation: "try_convert",
            item: "resource.divisibility",
            details: format!("Invalid divisibility: {}", e),
        })?;
        let metadata = deserialize_json(&self.metadata).map_err(|e| WalletStorageError::DecodingError {
            operation: "try_convert",
            item: "resource.metadata",
            details: e.to_string(),
        })?;
        let total_supply = self
            .total_supply
            .map(|s| {
                Amount::from_str(&s).map_err(|e| WalletStorageError::DecodingError {
                    operation: "try_convert",
                    item: "resource.total_supply",
                    details: e.to_string(),
                })
            })
            .transpose()?;
        let view_key = self.view_key.as_ref().map(deserialize_hex_try_from).transpose()?;
        let auth_hook = self.auth_hook.as_ref().map(deserialize_json).transpose().map_err(|e| {
            WalletStorageError::DecodingError {
                operation: "try_convert",
                item: "resource.auth_hook",
                details: e.to_string(),
            }
        })?;

        let resource = Resource::load(
            resource_type,
            owner_key,
            owner_rule,
            access_rules,
            metadata,
            view_key,
            auth_hook,
            divisibility,
            total_supply,
        );

        Ok(tari_ootle_wallet_sdk::models::ResourceModel { address, resource })
    }
}
