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

use rocksdb::ErrorKind;
use tari_ootle_common_types::optional::IsNotFoundError;
use tari_ootle_storage::StorageError;
use thiserror::Error;

use crate::codecs::EncodeVec;

#[derive(Debug, Error)]
pub enum RocksDbStorageError {
    #[error("General RocksDB error during operation {operation}: {source}")]
    RocksDbError {
        source: rocksdb::Error,
        operation: &'static str,
    },
    #[error("Entry {key:#} not found during operation {operation}")]
    NotFound {
        key: Box<EncodeVec>,
        operation: &'static str,
    },
    #[error("Encode error: {source}")]
    EncodeError { source: anyhow::Error },
    #[error("Encode error: {source}")]
    DecodeError { source: anyhow::Error },
    #[error("Malformed data: {operation}: {details}")]
    MalformedData { operation: &'static str, details: String },
    #[error("General error: {message}")]
    GeneralError { message: String },
    #[error("Conflicting insert: {details}")]
    ConflictingInsert { key: Box<EncodeVec>, details: String },
    #[error("[{operation}] Query error: {details}")]
    QueryError { operation: &'static str, details: String },
    #[error("[{operation}] Column family not found: {cf}")]
    ColumnFamilyNotFound { operation: &'static str, cf: String },
}

impl From<RocksDbStorageError> for StorageError {
    fn from(source: RocksDbStorageError) -> Self {
        match source {
            RocksDbStorageError::RocksDbError { source, operation } => match source.kind() {
                ErrorKind::NotFound => StorageError::NotFoundDbAdapter {
                    operation,
                    source: source.into(),
                },
                ErrorKind::Corruption |
                ErrorKind::NotSupported |
                ErrorKind::InvalidArgument |
                ErrorKind::IOError |
                ErrorKind::MergeInProgress |
                ErrorKind::Incomplete |
                ErrorKind::ShutdownInProgress |
                ErrorKind::TimedOut |
                ErrorKind::Aborted |
                ErrorKind::Busy |
                ErrorKind::Expired |
                ErrorKind::TryAgain |
                ErrorKind::CompactionTooLarge |
                ErrorKind::ColumnFamilyDropped |
                ErrorKind::Unknown => StorageError::General {
                    details: format!("{operation}: {source}"),
                },
            },
            RocksDbStorageError::NotFound { key, operation } => StorageError::NotFound {
                item: operation,
                key: format!("{:#}", key),
            },
            RocksDbStorageError::EncodeError { source } => StorageError::EncodingError {
                operation: "",
                item: "",
                details: source.to_string(),
            },
            RocksDbStorageError::DecodeError { source } => StorageError::DecodingError {
                operation: "unknown - rocks",
                item: "unknown",
                details: source.to_string(),
            },
            RocksDbStorageError::MalformedData { details, operation } => StorageError::DataInconsistency {
                details: format!("{operation}: {details}"),
            },
            RocksDbStorageError::QueryError { operation, details } => StorageError::QueryError {
                reason: format!("[{operation}] {details}"),
            },
            other => StorageError::General {
                details: other.to_string(),
            },
        }
    }
}

impl IsNotFoundError for RocksDbStorageError {
    fn is_not_found_error(&self) -> bool {
        match self {
            RocksDbStorageError::RocksDbError { source, .. } if source.kind() == ErrorKind::NotFound => true,
            RocksDbStorageError::NotFound { .. } => true,
            _ => false,
        }
    }
}
