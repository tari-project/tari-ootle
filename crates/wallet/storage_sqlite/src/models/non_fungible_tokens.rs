//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use tari_bor::Value;
use tari_ootle_wallet_sdk::storage::WalletStorageError;
use tari_template_lib_types::{NonFungibleId, ResourceAddress, VaultId};
use time::PrimitiveDateTime;

use crate::schema::non_fungible_tokens;

#[derive(Debug, Clone, Identifiable, Queryable)]
#[diesel(table_name = non_fungible_tokens)]
pub struct NonFungibleToken {
    pub id: i32,
    pub vault_id: i32,
    pub nft_id: String,
    pub resource_address: String,
    pub data: String,
    pub mutable_data: String,
    pub is_burnt: bool,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl NonFungibleToken {
    pub fn try_into_non_fungible_token(
        self,
        vault_id: VaultId,
    ) -> Result<tari_ootle_wallet_sdk::models::NonFungibleToken, WalletStorageError> {
        let data: Value = serde_json::from_str(&self.data).map_err(|e| WalletStorageError::DecodingError {
            operation: "try_from",
            item: "non_fungible_tokens.data",
            details: e.to_string(),
        })?;

        let mutable_data: Value =
            serde_json::from_str(&self.mutable_data).map_err(|e| WalletStorageError::DecodingError {
                operation: "try_from",
                item: "non_fungible_tokens.data",
                details: e.to_string(),
            })?;
        Ok(tari_ootle_wallet_sdk::models::NonFungibleToken {
            data,
            mutable_data,
            resource_address: ResourceAddress::from_str(&self.resource_address).map_err(|e| {
                WalletStorageError::DecodingError {
                    operation: "try_from",
                    item: "non_fungible_tokens.resource_address",
                    details: e.to_string(),
                }
            })?,
            nft_id: NonFungibleId::try_from_canonical_string(&self.nft_id).map_err(|e| {
                WalletStorageError::DecodingError {
                    operation: "try_from",
                    item: "non_fungible_tokens.nft_id",
                    details: format!("{:?}", e),
                }
            })?,
            vault_id,
            is_burnt: self.is_burnt,
        })
    }
}
