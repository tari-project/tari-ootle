//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::optional::IsNotFoundError;
use tari_template_lib::types::crypto::{RistrettoPublicKeyBytes, UtxoTag};

#[derive(Debug, thiserror::Error)]
pub enum WalletStorageError {
    #[error("General database failure for operation {operation}: {details}")]
    GeneralFailure { operation: &'static str, details: String },
    #[error("Bad query for operation {operation}: {details}")]
    BadQuery { operation: &'static str, details: String },
    #[error("Failed to decode for operation {operation} on {item}: {details}")]
    DecodingError {
        operation: &'static str,
        item: &'static str,
        details: String,
    },
    #[error("Failed to encode for operation {operation} on {item}: {details}")]
    EncodingError {
        operation: &'static str,
        item: &'static str,
        details: String,
    },
    #[error("[{operation}] {entity} not found with key {key}")]
    NotFound {
        operation: &'static str,
        entity: String,
        key: String,
    },
    #[error("Operation error {operation}: {details}")]
    OperationError { operation: &'static str, details: String },
    #[error("Data inconsistency for operation {operation}: {details}")]
    DataInconsistent { operation: &'static str, details: String },
    #[error("Encryption error {operation}: {details}")]
    EncryptionError { operation: &'static str, details: String },
    #[error("Decryption error {operation}: {details}")]
    DecryptionError { operation: &'static str, details: String },
}

impl IsNotFoundError for WalletStorageError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
}

impl WalletStorageError {
    pub fn general<E: std::fmt::Display>(operation: &'static str, e: E) -> Self {
        Self::GeneralFailure {
            operation,
            details: e.to_string(),
        }
    }

    pub fn bad_query<E: Into<String>>(operation: &'static str, details: E) -> Self {
        Self::BadQuery {
            operation,
            details: details.into(),
        }
    }

    pub fn not_found(operation: &'static str, entity: String, key: String) -> Self {
        Self::NotFound { operation, entity, key }
    }
}

pub type TagAndPublicNoncePair = (UtxoTag, RistrettoPublicKeyBytes);

pub trait CommittableStore {
    fn commit(&mut self) -> Result<(), WalletStorageError>;
    fn rollback(&mut self) -> Result<(), WalletStorageError>;
}
