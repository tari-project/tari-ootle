//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod account;
pub mod api_key;
pub mod confidential_output;
pub mod key_manager;
pub mod non_fungible_token;
pub mod resource;
pub mod stealth_output;
pub mod substate;
pub mod transaction;
pub mod vault;

pub use account::*;
pub use api_key::ApiKeyRecord;
pub use confidential_output::*;
pub use key_manager::*;
pub use non_fungible_token::*;
pub use resource::*;
pub use stealth_output::*;
pub use substate::*;
pub use transaction::*;
pub use vault::*;

use crate::storage::WalletStorageError;

// Re-exports used across the crate
pub use tari_ootle_wallet_sdk_types::*;

pub type Config<T> = crate::storage::Config<T>;

pub struct AddressBookEntry {
    pub id: i32,
    pub name: String,
    pub address: String,
    pub note: Option<String>,
}

pub struct WebauthnRegistrationPasskeyModel {
    pub passkey: webauthn_rs::prelude::Passkey,
}

impl TryFrom<&tari_ootle_wallet_storage_sqlite::models::WebauthnRegistrationPasskey>
    for WebauthnRegistrationPasskeyModel
{
    type Error = WalletStorageError;

    fn try_from(
        model: &tari_ootle_wallet_storage_sqlite::models::WebauthnRegistrationPasskey,
    ) -> Result<Self, Self::Error> {
        let passkey =
            serde_json::from_slice(&model.passkey).map_err(|e| WalletStorageError::DecodingError {
                operation: "webauthn_passkey",
                item: "passkey",
                details: e.to_string(),
            })?;
        Ok(Self { passkey })
    }
}

pub struct AuthoredTemplateModel {
    pub author_public_key: tari_template_lib::types::crypto::RistrettoPublicKeyBytes,
    pub address: tari_template_lib::types::TemplateAddress,
    pub name: String,
    pub abi_version: i32,
    pub functions: Vec<tari_engine_types::abi::TemplateDef>,
}
