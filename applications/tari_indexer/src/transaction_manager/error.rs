//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::optional::IsNotFoundError;
use tari_indexer_lib::{error::IndexerError, transaction_autofiller::TransactionAutofillerError};

use crate::network_client::NetworkClientError;

#[derive(Debug, thiserror::Error)]
pub enum TransactionManagerError {
    #[error(transparent)]
    NetworkClientError(#[from] NetworkClientError),
    #[error("{entity} not found: {key}")]
    NotFound { entity: &'static str, key: String },
    #[error(transparent)]
    SubstateScanningError(#[from] IndexerError),
    #[error(transparent)]
    TransactionAutofillerError(#[from] TransactionAutofillerError),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl IsNotFoundError for TransactionManagerError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
}
