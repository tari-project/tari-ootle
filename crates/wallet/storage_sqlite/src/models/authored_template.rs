// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::{models::AuthoredTemplateModel, storage::WalletStorageError};
use tari_template_abi::{FunctionDef, FunctionDefV1};
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
            functions: deserialize_functions(template.functions.as_str())?,
        })
    }
}

/// Reads a row's functions, stored in either the V1 layout (with `is_mut`) or the current one.
///
/// V1 is tried first. It requires `is_mut`, which the current layout lacks, while the current layout's optional
/// `receiver` would also accept a V1 row and leave its `self` argument in place.
fn deserialize_functions(json: &str) -> Result<Vec<FunctionDef>, WalletStorageError> {
    if let Ok(functions) = serde_json::from_str::<Vec<FunctionDefV1>>(json) {
        return Ok(functions.into_iter().map(Into::into).collect());
    }
    deserialize_json(json)
}

#[cfg(test)]
mod tests {
    use tari_template_abi::{Receiver, Type};

    use super::*;

    #[test]
    fn a_v1_row_reads_with_its_receiver_lifted_out_of_the_arguments() {
        let json = r#"[{"name":"get","arguments":[{"name":"self","arg_type":{"Other":{"name":"&self"}}},{"name":"key","arg_type":"U64"}],"output":"Unit","is_mut":false,"is_migration":false}]"#;

        let functions = deserialize_functions(json).unwrap();

        assert_eq!(functions[0].receiver, Some(Receiver::Ref));
        assert_eq!(functions[0].arguments.len(), 1);
        assert_eq!(functions[0].arguments[0].name, "key");
    }

    #[test]
    fn a_current_row_reads_unchanged() {
        let function = FunctionDef {
            name: "add".to_string(),
            receiver: Some(Receiver::RefMut),
            arguments: vec![],
            output: Type::Unit,
            is_migration: false,
        };
        let json = serde_json::to_string(&vec![function]).unwrap();

        let functions = deserialize_functions(&json).unwrap();

        assert_eq!(functions[0].receiver, Some(Receiver::RefMut));
        assert!(functions[0].arguments.is_empty());
    }
}
