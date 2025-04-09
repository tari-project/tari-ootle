// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use chrono::NaiveDateTime;
use tari_dan_wallet_sdk::{models::AuthoredTemplateModel, storage::WalletStorageError};
use tari_template_lib::types::TemplateAddress;

use crate::schema::authored_templates;

#[derive(Debug, Clone, Queryable, Identifiable, Selectable)]
#[diesel(table_name = authored_templates)]
pub struct AuthoredTemplate {
    pub id: i32,
    pub key_index: i32,
    pub address: String,
    pub name: String,
    pub tari_version: String,
    pub functions: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl TryFrom<AuthoredTemplateModel> for AuthoredTemplate {
    type Error = serde_json::Error;

    fn try_from(model: AuthoredTemplateModel) -> Result<Self, Self::Error> {
        Ok(Self {
            id: 0,
            key_index: model.key_index as i32,
            address: format!("{}", model.address),
            name: model.name,
            tari_version: model.tari_version,
            functions: serde_json::to_string(&model.functions)?,
            created_at: Default::default(),
            updated_at: Default::default(),
        })
    }
}

impl TryFrom<&AuthoredTemplate> for AuthoredTemplateModel {
    type Error = WalletStorageError;

    fn try_from(template: &AuthoredTemplate) -> Result<Self, Self::Error> {
        Ok(Self {
            key_index: template.key_index as u64,
            address: TemplateAddress::from_hex(template.address.as_str()).map_err(|error| {
                Self::Error::DecodingError {
                    operation: "convert_authored_template_to_model",
                    item: "address",
                    details: error.to_string(),
                }
            })?,
            name: template.name.clone(),
            tari_version: template.tari_version.clone(),
            functions: serde_json::from_str(template.functions.as_str()).map_err(|error| {
                Self::Error::DecodingError {
                    operation: "convert_authored_template_to_model",
                    item: "functions",
                    details: error.to_string(),
                }
            })?,
        })
    }
}
