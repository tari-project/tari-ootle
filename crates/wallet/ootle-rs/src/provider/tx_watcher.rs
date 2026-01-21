//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    mem,
    pin::{pin, Pin},
    sync::{Arc, Weak},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures::{stream::BoxStream, FutureExt, StreamExt};
use tari_indexer_client::{
    error::IndexerRestClientError,
    event::TransactionFinalizedEvent,
    rest_api_client::IndexerRestApiClient,
    sse,
    types::{GetTransactionResultRequest, IndexerTransactionFinalizedResult},
};
use tari_ootle_common_types::{
    engine_types::{
        commit_result::TransactionResult,
        transaction_receipt::{FinalizeOutcome, TransactionReceipt},
    },
    optional::Optional,
};
use tari_ootle_transaction::TransactionId;
use tokio::{
    sync::{mpsc, oneshot},
    task,
    time::sleep_until,
};

use crate::{
    provider::tx_stream::{EventStreamError, Paused},
    TransactionOutcome,
};

pub struct TransactionWatcher {
    stream: BoxStream<'static, Result<sse::Event, EventStreamError>>,
    span: tracing::Span,
    pending_requests: HashMap<TransactionId, TxWatchRequest>,
    reap_times: BTreeMap<Instant, TransactionId>,
    paused: Paused,
}

impl TransactionWatcher {
    pub(crate) fn new(stream: BoxStream<'static, Result<sse::Event, EventStreamError>>, paused: Paused) -> Self {
        let span = tracing::span!(tracing::Level::DEBUG, "TransactionWatcher::new");
        Self {
            stream,
            span,
            pending_requests: HashMap::new(),
            reap_times: BTreeMap::new(),
            paused,
        }
    }

    pub(crate) fn spawn(self) -> TransactionWatcherHandle {
        let (tx_requests, rx_requests) = mpsc::channel(2);
        task::spawn(self.run(rx_requests));
        TransactionWatcherHandle::new(tx_requests)
    }

    /// Returns the next time a transaction watch request is set to be reaped, or if there are none, a time far in the
    /// future i.e. don't wake up.
    fn next_reap_time(&self) -> Instant {
        self.reap_times
            .first_key_value()
            .map(|(instant, _)| *instant)
            .unwrap_or(Instant::now() + Duration::from_secs(60_000))
    }

    async fn run(mut self, mut rx_requests: mpsc::Receiver<TxWatchRequest>) {
        loop {
            self.update_pause_state();

            let next_reap_time = self.next_reap_time();
            let sleep = pin!(sleep_until(next_reap_time.into()));

            tokio::select! {
                Some(request) =  rx_requests.recv() => {
                    self.handle_request(request).await;
                },

                Some(event) = self.stream.next() => {
                    self.handle_event(event).await;
                }

                // Wake up to reap timed out requests even if there are no other events/requests.
                _ = sleep => {}
            }

            self.reap_timeouts();
        }
    }

    fn reap_timeouts(&mut self) {
        let to_keep = self.reap_times.split_off(&Instant::now());
        let to_reap = mem::replace(&mut self.reap_times, to_keep);
        for tx_id in to_reap.into_values() {
            if let Some(request) = self.pending_requests.remove(&tx_id) {
                tracing::warn!("Transaction watch timed out for tx_id: {}", tx_id);
                let _ignore = request
                    .reply
                    .send(Err(PendingTransactionError::Timeout { tx_id }))
                    .inspect_err(|_| {
                        tracing::error!("Failed to send timeout notification for tx_id: {}", tx_id);
                    });
            }
        }
    }

    async fn handle_request(&mut self, request: TxWatchRequest) {
        let _enter = self.span.enter();
        tracing::debug!("Received watch request for tx_id: {}", request.tx_id);
        self.reap_times.insert(Instant::now() + request.timeout, request.tx_id);
        self.pending_requests.insert(request.tx_id, request);
    }

    async fn handle_event(&mut self, event: Result<sse::Event, EventStreamError>) {
        const TX_FINALIZED_EVENT_TYPE: &str = "TransactionFinalized";
        let _enter = self.span.enter();
        match event {
            Ok(event) => {
                tracing::debug!("Received SSE event: {:?}", event);
                if event.event_type != TX_FINALIZED_EVENT_TYPE {
                    tracing::debug!("Ignoring non-finalized event of type: {}", event.event_type);
                    return;
                }

                // Parse the event data to extract transaction ID and outcome
                let event_data: TransactionFinalizedEvent = match serde_json::from_str(&event.data) {
                    Ok(data) => data,
                    Err(e) => {
                        tracing::error!("Failed to parse event data: {}", e);
                        return;
                    },
                };

                let Some(watch) = self.pending_requests.remove(&event_data.transaction_id) else {
                    tracing::debug!("No pending watch found for tx_id: {}", event_data.transaction_id);
                    return;
                };

                tracing::info!(
                    "Transaction {} finalized with outcome: {:?}",
                    event_data.transaction_id,
                    event_data.outcome
                );

                watch.reply.send(Ok(event_data.outcome)).unwrap_or_else(|_| {
                    tracing::error!(
                        "Failed to send transaction outcome for tx_id: {}",
                        event_data.transaction_id
                    );
                });
            },
            Err(e) => {
                tracing::error!("Error receiving SSE event: {}", e);
            },
        }
    }

    fn update_pause_state(&self) {
        let should_pause = self.pending_requests.is_empty();
        if self.paused.set_paused(should_pause) {
            if should_pause {
                tracing::debug!("No pending transactions. Pausing event stream.");
            } else {
                tracing::debug!("Pending transactions exist. Resuming event stream.");
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PendingTransactionError {
    #[error("Client has been dropped")]
    ClientDropped,
    #[error("Indexer client error: {0}")]
    IndexerClientError(#[from] IndexerRestClientError),
    #[error("The transaction watch was aborted for tx_id: {tx_id}")]
    WatchAborted { tx_id: TransactionId },
    #[error("Transaction receipt not found for tx_id: {tx_id}")]
    ReceiptNotFound { tx_id: TransactionId },
    #[error("Transaction {tx_id} was rejected: {reason}")]
    TransactionRejected { tx_id: TransactionId, reason: String },
    #[error("Transaction timed out for tx_id: {tx_id}")]
    Timeout { tx_id: TransactionId },
}

impl PendingTransactionError {
    pub fn is_timeout(&self) -> bool {
        matches!(self, PendingTransactionError::Timeout { .. })
    }
}

#[derive(Debug)]
pub struct TxWatchRequest {
    pub tx_id: TransactionId,
    pub timeout: Duration,
    pub reply: oneshot::Sender<Result<FinalizeOutcome, PendingTransactionError>>,
}

#[derive(Debug, Clone)]
pub struct TransactionWatcherHandle {
    tx_requests: mpsc::Sender<TxWatchRequest>,
}

impl TransactionWatcherHandle {
    pub fn new(tx_requests: mpsc::Sender<TxWatchRequest>) -> Self {
        Self { tx_requests }
    }

    pub async fn watch_transaction(&self, tx_id: TransactionId, timeout: Duration) -> PendingTransaction {
        let (tx_reply, rx_reply) = oneshot::channel();
        let request = TxWatchRequest {
            tx_id,
            timeout,
            reply: tx_reply,
        };
        self.tx_requests
            .send(request)
            .await
            .expect("TransactionWatcher not alive");
        PendingTransaction {
            tx_id,
            outcome_rx: rx_reply,
        }
    }
}

pub struct PendingTransactionHandle {
    watcher: TransactionWatcherHandle,
    client: Weak<IndexerRestApiClient>,
    tx_id: TransactionId,
    default_timeout: Duration,
}

impl PendingTransactionHandle {
    pub fn new(watcher: TransactionWatcherHandle, client: Weak<IndexerRestApiClient>, tx_id: TransactionId) -> Self {
        const DEFAULT_TX_TIMEOUT: Duration = Duration::from_secs(30);
        Self {
            watcher,
            default_timeout: DEFAULT_TX_TIMEOUT,
            client,
            tx_id,
        }
    }

    pub fn with_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.default_timeout = timeout;
        self
    }

    pub fn tx_id(&self) -> TransactionId {
        self.tx_id
    }

    pub async fn register(&self, timeout: Duration) -> Result<PendingTransaction, PendingTransactionError> {
        let pending = self.watcher.watch_transaction(self.tx_id, timeout).await;
        Ok(pending)
    }

    pub async fn watch(&self) -> Result<TransactionOutcome, PendingTransactionError> {
        match self.register(self.default_timeout).await?.await {
            Ok(outcome) => Ok(outcome.into()),
            Err(PendingTransactionError::Timeout { .. }) => {
                tracing::warn!("Transaction watch timed out, attempting direct query: {}", self.tx_id);
                match self.try_get_transaction_result().await {
                    Ok(Some(IndexerTransactionFinalizedResult::Pending)) => {
                        tracing::warn!("Transaction is still pending after timeout: {}", self.tx_id);
                        Err(PendingTransactionError::Timeout { tx_id: self.tx_id })
                    },
                    Ok(Some(IndexerTransactionFinalizedResult::Finalized {
                        execution_result,
                        abort_details,
                        ..
                    })) => {
                        if let Some(result) = execution_result {
                            return match result.finalize.result {
                                TransactionResult::Accept(_) => Ok(TransactionOutcome::Commit),
                                TransactionResult::AcceptFeeRejectRest(_, _) => Ok(TransactionOutcome::OnlyFeeCommit),
                                TransactionResult::Reject(reason) => Ok(TransactionOutcome::Reject(reason)),
                            };
                        }

                        // Any commit case would have been handled above, so this is a rejection
                        let reason = abort_details.unwrap_or_else(|| "Unknown".to_string());
                        Err(PendingTransactionError::TransactionRejected {
                            tx_id: self.tx_id,
                            reason,
                        })
                    },
                    Ok(None) => {
                        tracing::error!("Transaction result not found after timeout: {}", self.tx_id);
                        Err(PendingTransactionError::Timeout { tx_id: self.tx_id })
                    },
                    Err(e) => {
                        tracing::error!("Failed to get transaction result after timeout: {}", e);
                        Err(PendingTransactionError::Timeout { tx_id: self.tx_id })
                    },
                }
            },
            Err(e) => Err(e),
        }
    }

    async fn try_get_transaction_result(
        &self,
    ) -> Result<Option<IndexerTransactionFinalizedResult>, PendingTransactionError> {
        let client = self.upgrade_client()?;
        let resp = client
            .get_transaction_result(GetTransactionResultRequest {
                transaction_id: self.tx_id,
            })
            .await
            .optional()?;
        Ok(resp.map(|r| r.result))
    }

    async fn try_get_transaction_receipt(&self) -> Result<Option<TransactionReceipt>, PendingTransactionError> {
        let client = self.upgrade_client()?;
        if let Some(receipt) = client
            .get_transaction_receipt(self.tx_id.into_receipt_address())
            .await
            .optional()?
        {
            return Ok(Some(receipt.receipt));
        }
        Ok(None)
    }

    fn upgrade_client(&self) -> Result<Arc<IndexerRestApiClient>, PendingTransactionError> {
        let client = self.client.upgrade().ok_or(PendingTransactionError::ClientDropped)?;
        Ok(client)
    }

    pub async fn get_receipt(&self) -> Result<TransactionReceipt, PendingTransactionError> {
        if let Some(receipt) = self.try_get_transaction_receipt().await? {
            return Ok(receipt);
        }
        if let Some(IndexerTransactionFinalizedResult::Finalized {
            final_decision,
            abort_details,
            execution_result,
            ..
        }) = self.try_get_transaction_result().await?
        {
            if final_decision.is_abort() {
                let reason = execution_result
                    .as_ref()
                    .and_then(|res| res.finalize.result.any_reject())
                    .map(|reject| reject.to_string())
                    .or(abort_details)
                    .unwrap_or_else(|| "Unknown".to_string());
                return Err(PendingTransactionError::TransactionRejected {
                    tx_id: self.tx_id,
                    reason,
                });
            }
            if final_decision.is_commit() {
                // Transaction committed but receipt not found. This is a due to a race condition where the indexer may
                // not have indexed the receipt yet.
                //
                // TODO: improvements to the indexer may be needed to fully resolve this.
                tracing::warn!("Transaction committed but receipt not found for tx_id: {}", self.tx_id);
                return Ok(TransactionReceipt {
                    outcome: FinalizeOutcome::Commit,
                    diff_summary: execution_result
                        .as_ref()
                        .and_then(|res| res.finalize.any_accept())
                        .map(Into::into)
                        .unwrap_or_default(),
                    fee_withdrawals: execution_result
                        .as_ref()
                        .and_then(|res| res.finalize.any_accept())
                        .map(|diff| diff.validator_fee_withdrawals().to_vec().into_boxed_slice())
                        .unwrap_or_default(),
                    events: execution_result
                        .as_ref()
                        .map(|res| res.finalize.events.clone().into_boxed_slice())
                        .unwrap_or_default(),
                    logs: execution_result
                        .as_ref()
                        .map(|res| res.finalize.logs.clone().into_boxed_slice())
                        .unwrap_or_default(),
                    fee_receipt: execution_result
                        .as_ref()
                        .map(|res| res.finalize.fee_receipt.clone())
                        .unwrap_or_default(),
                    epoch: execution_result
                        .as_ref()
                        .and_then(|res| res.execute_epoch)
                        .unwrap_or_default(),
                });

                // return Err(PendingTransactionError::ReceiptNotFound { tx_id: self.tx_id });
            }
        }

        let _outcome = self.watch().await?;

        let receipt = self
            .try_get_transaction_receipt()
            .await?
            .ok_or_else(|| PendingTransactionError::ReceiptNotFound { tx_id: self.tx_id })?;

        Ok(receipt)
    }
}

pub struct PendingTransaction {
    tx_id: TransactionId,
    outcome_rx: oneshot::Receiver<Result<FinalizeOutcome, PendingTransactionError>>,
}

impl Future for PendingTransaction {
    type Output = Result<FinalizeOutcome, PendingTransactionError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.outcome_rx
            .poll_unpin(cx)
            .map(|res| res.unwrap_or_else(|_| Err(PendingTransactionError::WatchAborted { tx_id: self.tx_id })))
    }
}

impl PendingTransaction {
    pub fn tx_id(&self) -> TransactionId {
        self.tx_id
    }
}
