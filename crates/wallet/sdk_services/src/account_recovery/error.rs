//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_wallet_sdk::{
    apis::{accounts::AccountsApiError, key_manager::KeyManagerApiError},
    storage::WalletStorageError,
};

use crate::account_monitor::AccountMonitorError;

#[derive(Debug, thiserror::Error)]
pub enum AccountRecoveryError {
    #[error("Wallet store error: {0}")]
    WalletStoreError(#[from] WalletStorageError),
    #[error("Key manager API error: {0}")]
    KeyManagerApiError(#[from] KeyManagerApiError),
    #[error("Accounts API error: {0}")]
    AccountsApiError(#[from] AccountsApiError),
    #[error("Network interface error: {details}")]
    NetworkInterfaceError { details: String },
    #[error("Account monitor error: {0}")]
    AccountMonitorError(#[from] AccountMonitorError),
    #[error("Invalid response: {details}")]
    InvalidResponse { details: String },
}
