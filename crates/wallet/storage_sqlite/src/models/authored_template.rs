// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::{models::AuthoredTemplateModel, storage::WalletStorageError};
use time::PrimitiveDateTime;

use crate::{
    schema::authored_templates,
    serialization::{deserialize_hex_try_from, deserialize_json},
};

#[derive(Debug, Clone, Queryable, Identifiable, Selectable)]
#[diesel(table_name = authored_templates)]
pub struct AuthoredTemplate {
    pub id: i32,
    pub author_public_key: String,
    pub address: String,
    pub name: String,
    pub abi_version: i32,
    pub functions: String,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl TryFrom<&AuthoredTemplate> for AuthoredTemplateModel {
    type Error = WalletStorageError;

    fn try_from(template: &AuthoredTemplate) -> Result<Self, Self::Error> {
        Ok(Self {
            author_public_key: deserialize_hex_try_from(&template.author_public_key)?,
            address: deserialize_hex_try_from(template.address.as_str())?,
            name: template.name.clone(),
            abi_version: template
                .abi_version
                .try_into()
                .map_err(|e| WalletStorageError::EncodingError {
                    operation: "AuthoredTemplateModel::try_from",
                    item: "abi_version",
                    details: format!("Failed to convert abi_version: {}", e),
                })?,
            functions: deserialize_json(template.functions.as_str())?,
        })
    }
}
