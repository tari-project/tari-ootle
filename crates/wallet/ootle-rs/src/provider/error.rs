//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_indexer_client::error::IndexerRestClientError;

use crate::{provider::input_resolver::TransactionInputResolverError, wallet::WalletError};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Indexer client error: {0}")]
    IndexerClientError(#[from] IndexerRestClientError),
    #[error("Transaction encoding error: {source}")]
    TransactionEncodeError {
        #[from]
        source: tari_bor::BorError,
    },
    #[error("Transaction input resolution error: {error}")]
    TransactionInputResolutionError {
        #[from]
        error: TransactionInputResolverError,
    },
    #[error(transparent)]
    WalletError(#[from] WalletError),
}
