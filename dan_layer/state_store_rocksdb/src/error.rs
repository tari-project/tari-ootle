//  Copyright 2024. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::default;

use tari_dan_common_types::optional::IsNotFoundError;
use tari_dan_storage::StorageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RocksDbStorageError {
    #[error("General RocksDB error during operation {operation}: {source}")]
    RocksDbError {
        source: rocksdb::Error,
        operation: &'static str,
    },
    #[error("Entry {key} not found during operation {operation}")]
    NotFound { key: String, operation: &'static str },
    #[error("Encode error: {source}")]
    EncodeError {
        #[from]
        source: bincode::error::EncodeError,
    },
    #[error("Encode error: {source}")]
    DecodeError {
        #[from]
        source: bincode::error::DecodeError,
    },
    #[error("General error: {message}")]
    GeneralError {
        message: String,
    },

    /*
    #[error("Could not connect to database: {source}")]
    ConnectionError {
        #[from]
        source: diesel::ConnectionError,
    },
    #[error("General diesel error during operation {operation}: {source}")]
    DieselError {
        source: diesel::result::Error,
        operation: &'static str,
    },
    #[error("Could not migrate the database")]
    MigrationError {
        #[from]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("Malformed DB data in {operation}: {details}")]
    MalformedDbData { operation: &'static str, details: String },
    #[error("Database inconsistency for operation {operation}: {details}")]
    DbInconsistency { operation: &'static str, details: String },
    #[error("[{operation}] Not all queried transactions were found: {details}")]
    NotAllTransactionsFound { operation: &'static str, details: String },
    #[error("[{operation}] Not all queried substates were found: {details}")]
    NotAllSubstatesFound { operation: &'static str, details: String },
    #[error("[{operation}] Not all {items} were found: {details}")]
    NotAllItemsFound {
        items: &'static str,
        operation: &'static str,
        details: String,
    },
     */
}

impl From<RocksDbStorageError> for StorageError {
    fn from(source: RocksDbStorageError) -> Self {
        match source {
            RocksDbStorageError::RocksDbError { .. } => StorageError::QueryError {
                reason: source.to_string(),
            },
            RocksDbStorageError::NotFound { key, operation } => StorageError::NotFound { item: operation, key },
            RocksDbStorageError::EncodeError { source } => StorageError::EncodingError { operation: "", item: "", details: source.to_string() },
            RocksDbStorageError::DecodeError { source } => StorageError::DecodingError { operation: "", item: "", details: source.to_string() },
            RocksDbStorageError::GeneralError { .. } => StorageError::General { details: source.to_string() },
            /*
            RocksDbStorageError::ConnectionError { .. } => StorageError::ConnectionError {
                reason: source.to_string(),
            },
            RocksDbStorageError::DieselError { source, operation } if matches!(source, diesel::NotFound) => {
                StorageError::NotFoundDbAdapter {
                    operation,
                    source: anyhow::anyhow!(source),
                }
            },
            RocksDbStorageError::DieselError { .. } => StorageError::QueryError {
                reason: source.to_string(),
            },
            RocksDbStorageError::MigrationError { .. } => StorageError::MigrationError {
                reason: source.to_string(),
            },
            other => StorageError::General {
                details: other.to_string(),
            },
            */
        }
    }
}

impl IsNotFoundError for RocksDbStorageError {
    fn is_not_found_error(&self) -> bool {
        todo!()
        /*
        matches!(self, RocksDbStorageError::DieselError { source, .. } if matches!(source, diesel::result::Error::NotFound))
         */
    }
}
