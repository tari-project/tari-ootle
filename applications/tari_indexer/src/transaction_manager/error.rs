//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_indexer_lib::error::IndexerError;
use tari_ootle_common_types::optional::IsNotFoundError;
use tari_ootle_storage::StorageError;
use tari_ootle_transaction::TransactionId;

use crate::network_client::NetworkClientError;

#[derive(Debug, thiserror::Error)]
pub enum TransactionManagerError {
    #[error("Transaction {transaction_id} is invalid: {details}")]
    InvalidTransaction {
        transaction_id: TransactionId,
        details: String,
    },
    #[error(transparent)]
    NetworkClientError(Box<NetworkClientError>),
    #[error("{entity} not found: {key}")]
    NotFound { entity: &'static str, key: String },
    #[error(transparent)]
    SubstateScanningError(#[from] IndexerError),
    #[error("Store error: {0}")]
    StoreError(#[from] StorageError),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl IsNotFoundError for TransactionManagerError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
}

impl From<NetworkClientError> for TransactionManagerError {
    fn from(err: NetworkClientError) -> Self {
        Self::NetworkClientError(Box::new(err))
    }
}
