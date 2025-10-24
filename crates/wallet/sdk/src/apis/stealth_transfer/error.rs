//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::optional::IsNotFoundError;
use tari_template_lib::models::ResourceAddress;

use crate::{
    apis::{
        accounts::AccountsApiError,
        config::ConfigApiError,
        key_manager::KeyManagerApiError,
        stealth_crypto::StealthCryptoApiError,
        stealth_outputs::StealthOutputsApiError,
        substate::SubstateApiError,
    },
    storage::WalletStorageError,
};

#[derive(Debug, thiserror::Error)]
pub enum StealthTransferApiError {
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Confidential crypto error: {0}")]
    Crypto(#[from] StealthCryptoApiError),
    #[error("Stealth outputs error: {0}")]
    OutputsApi(#[from] StealthOutputsApiError),
    #[error("Substate API error: {0}")]
    SubstateApi(#[from] SubstateApiError),
    #[error("Key manager error: {0}")]
    KeyManagerApi(#[from] KeyManagerApiError),
    #[error("Insufficient funds")]
    InsufficientFunds,
    #[error("Badge vault not found for resource {resource_address}")]
    BadgeVaultNotFound { resource_address: ResourceAddress },
    #[error("Accounts API error: {0}")]
    Accounts(#[from] AccountsApiError),
    #[error("Invalid parameter `{param}`: {reason}")]
    InvalidParameter { param: &'static str, reason: String },
    #[error("Unexpected indexer response: {details}")]
    UnexpectedIndexerResponse { details: String },
    #[error("Config API error: {0}")]
    ConfigApi(#[from] ConfigApiError),
    #[error("Amount overflow for parameter `{param}`: {details}")]
    AmountOverflow { param: &'static str, details: String },
    #[error("Insufficient revealed funds: {details}")]
    InsufficientRevealedFunds { details: String },
}

impl IsNotFoundError for StealthTransferApiError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::StoreError(e) if e.is_not_found_error() )
    }
}
