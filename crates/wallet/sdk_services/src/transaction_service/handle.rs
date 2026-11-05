//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::commit_result::ExecuteResult;
use tari_ootle_transaction::{Transaction, TransactionId};
use tari_ootle_wallet_sdk::models::{TransactionContext, WalletLockId};
use tokio::sync::{mpsc, oneshot};

use super::TransactionServiceError;
use crate::Reply;

#[derive(Debug)]
pub(super) enum TransactionServiceRequest {
    SubmitTransaction {
        transaction: Transaction,
        context: Option<TransactionContext>,
        lock_id: Option<WalletLockId>,
        reply: Reply<Result<TransactionId, TransactionServiceError>>,
    },

    SubmitDryRunTransaction {
        transaction: Transaction,
        reply: Reply<Result<ExecuteResult, TransactionServiceError>>,
    },
}

#[derive(Debug, Clone)]
pub struct TransactionServiceHandle {
    sender: mpsc::Sender<TransactionServiceRequest>,
}

impl TransactionServiceHandle {
    pub(super) fn new(sender: mpsc::Sender<TransactionServiceRequest>) -> Self {
        Self { sender }
    }
}

impl TransactionServiceHandle {
    pub async fn submit_transaction(&self, transaction: Transaction) -> Result<TransactionId, TransactionServiceError> {
        self.submit_transaction_with_opts(transaction, None, None).await
    }

    pub async fn submit_dry_run_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<ExecuteResult, TransactionServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(TransactionServiceRequest::SubmitDryRunTransaction {
                transaction,
                reply: reply_tx,
            })
            .await
            .map_err(|_| TransactionServiceError::ServiceShutdown)?;
        reply_rx.await.map_err(|_| TransactionServiceError::ServiceShutdown)?
    }

    pub async fn submit_transaction_with_opts(
        &self,
        transaction: Transaction,
        context: Option<TransactionContext>,
        lock_id: Option<WalletLockId>,
    ) -> Result<TransactionId, TransactionServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(TransactionServiceRequest::SubmitTransaction {
                transaction,
                context,
                lock_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| TransactionServiceError::ServiceShutdown)?;
        reply_rx.await.map_err(|_| TransactionServiceError::ServiceShutdown)?
    }
}
