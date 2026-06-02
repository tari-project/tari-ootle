//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{sync::Arc, time::Duration};

use log::*;
use tari_engine_types::commit_result::ExecuteResult;
use tari_ootle_common_types::{optional::IsNotFoundError, response_status::TransactionStatusResponseError};
use tari_ootle_transaction::{Transaction, TransactionId};
use tari_ootle_wallet_sdk::{
    WalletSdk,
    WalletSdkSpec,
    models::{
        TransactionContext,
        TransactionContextKind,
        TransactionFinalizedEvent,
        TransactionInvalidEvent,
        TransactionStatus,
        TransactionSubmittedEvent,
        WalletEvent,
        WalletLockId,
    },
    network::WalletNetworkInterface,
};
use tari_shutdown::ShutdownSignal;
use tokio::{
    sync::{Semaphore, mpsc, watch},
    time,
    time::MissedTickBehavior,
};

use super::{
    error::TransactionServiceError,
    handle::{TransactionServiceHandle, TransactionServiceRequest},
};
use crate::notify::Notify;

const LOG_TARGET: &str = "tari::ootle::wallet_services::transaction_service";

pub struct TransactionService<TSpec: WalletSdkSpec> {
    rx_request: mpsc::Receiver<TransactionServiceRequest>,
    notify: Notify<WalletEvent>,
    wallet_sdk: WalletSdk<TSpec>,
    trigger_poll: watch::Sender<()>,
    rx_trigger: watch::Receiver<()>,
    poll_semaphore: Arc<Semaphore>,
    shutdown_signal: ShutdownSignal,
}

impl<TSpec> TransactionService<TSpec>
where
    TSpec: WalletSdkSpec + Send + 'static,
    TSpec::Store: Clone + Send + Sync + 'static,
    TSpec::NetworkInterface: Clone + Send + Sync + 'static,
    TSpec::KeyStore: Clone + Send + Sync + 'static,
    <TSpec::NetworkInterface as WalletNetworkInterface>::Error: IsNotFoundError + TransactionStatusResponseError,
{
    pub fn new(
        notify: Notify<WalletEvent>,
        wallet_sdk: WalletSdk<TSpec>,
        shutdown_signal: ShutdownSignal,
    ) -> (Self, TransactionServiceHandle) {
        let (trigger, rx_trigger) = watch::channel(());
        let (tx_request, rx_request) = mpsc::channel(1);
        let actor = Self {
            rx_request,
            notify,
            wallet_sdk,
            trigger_poll: trigger,
            rx_trigger,
            poll_semaphore: Arc::new(Semaphore::new(1)),
            shutdown_signal,
        };

        (actor, TransactionServiceHandle::new(tx_request))
    }

    pub async fn run(mut self) -> Result<(), anyhow::Error> {
        let mut events_subscription = self.notify.subscribe();
        let mut poll_interval = time::interval(Duration::from_secs(5));
        poll_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = self.shutdown_signal.wait() => {
                    break Ok(());
                }
                Some(req) = self.rx_request.recv() => {
                    if let Err(err) = self.handle_request(req).await {
                        error!(target: LOG_TARGET, "Error handling request: {}", err);
                    }
                },
                Ok(event) = events_subscription.recv() => {
                    if let Err(e) = self.on_event(event) {
                        error!(target: LOG_TARGET, "Error handling event: {}", e);
                    }
                },

                Ok(_) = self.rx_trigger.changed() => {
                    trace!(target: LOG_TARGET, "Polling for transactions");
                    // Wait a tick for the transaction to be processed. If we poll immediately, the transaction is very likely not
                    // to be finalised yet, then we have to wait for the next tick. This improves the perception of the finalisation time
                    // if the transaction is finalised within 750ms.
                    poll_interval.reset_after(Duration::from_millis(750));
                }

                _ = poll_interval.tick() => {
                    trace!(target: LOG_TARGET, "Polling for transactions");
                    self.on_poll().await?;
                }
            }
        }
    }

    async fn handle_request(&self, request: TransactionServiceRequest) -> Result<(), TransactionServiceError> {
        match request {
            TransactionServiceRequest::SubmitTransaction {
                transaction,
                context,
                lock_id,
                reply,
            } => {
                reply
                    .send(self.handle_submit_transaction(transaction, context, lock_id).await)
                    .map_err(|_| TransactionServiceError::ServiceShutdown)?;
            },
            TransactionServiceRequest::SubmitDryRunTransaction { transaction, reply } => {
                let transaction_id = transaction.calculate_id();
                let transaction_api = self.wallet_sdk.transaction_api();
                // Unlock all locks related to the transaction immediately since this is a dry run
                transaction_api.release_all_locks_for_transaction(transaction_id)?;
                match transaction_api.submit_dry_run_transaction(transaction).await {
                    Ok(finalized_transaction) => {
                        let finalize =
                            finalized_transaction
                                .finalize
                                .ok_or_else(|| TransactionServiceError::InvariantError {
                                    details: format!(
                                        "Dry run transaction {transaction_id} succeeded but was not finalized"
                                    ),
                                });
                        reply
                            .send(finalize.map(|finalize| ExecuteResult {
                                finalize,
                                execution_time: finalized_transaction.execution_time.unwrap_or_default(),
                                execute_epoch: None,
                            }))
                            .map_err(|_| TransactionServiceError::ServiceShutdown)?;
                    },
                    Err(e) => {
                        reply
                            .send(Err(e.into()))
                            .map_err(|_| TransactionServiceError::ServiceShutdown)?;
                    },
                }
            },
        }
        Ok(())
    }

    async fn handle_submit_transaction(
        &self,
        transaction: Transaction,
        context: Option<TransactionContext>,
        lock_id: Option<WalletLockId>,
    ) -> Result<TransactionId, TransactionServiceError> {
        let transaction_api = self.wallet_sdk.transaction_api();
        let new_account_info = context.as_ref().and_then(|c| c.new_account_data()).cloned();
        let linked_accounts = context
            .as_ref()
            .map(|c| c.linked_accounts.as_slice())
            .unwrap_or_default();
        let transaction_id =
            transaction_api.insert_new_transaction(transaction, new_account_info, linked_accounts, false)?;

        if let Some(lock_id) = lock_id {
            transaction_api.locks_set_transaction_id(lock_id, transaction_id)?;
        }

        if transaction_api.submit_transaction(transaction_id).await? {
            self.notify.notify(TransactionSubmittedEvent {
                transaction_id,
                context,
            });
            Ok(transaction_id)
        } else {
            self.notify.notify(TransactionInvalidEvent {
                transaction_id,
                status: TransactionStatus::InvalidTransaction,
                finalize: None,
                final_fee: None,
            });
            Ok(transaction_id)
        }
    }

    async fn on_poll(&self) -> Result<(), TransactionServiceError> {
        let permit = match self.poll_semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                debug!(target: LOG_TARGET, "Polling is already in progress");
                return Ok(());
            },
        };

        let wallet_sdk = self.wallet_sdk.clone();
        let notify = self.notify.clone();
        tokio::spawn(async move {
            if let Err(err) = Self::resubmit_new_transactions(&wallet_sdk, &notify).await {
                error!(target: LOG_TARGET, "Error resubmitting new transactions: {}", err);
            }
            if let Err(err) = Self::check_pending_transactions(&wallet_sdk, &notify).await {
                error!(target: LOG_TARGET, "Error checking pending transactions: {}", err);
            }
            if let Err(err) = Self::clear_stale_locks(&wallet_sdk) {
                error!(target: LOG_TARGET, "Error clearing stale locks: {}", err);
            }

            drop(permit);
        });
        Ok(())
    }

    fn clear_stale_locks(wallet_sdk: &WalletSdk<TSpec>) -> Result<(), TransactionServiceError> {
        let transaction_api = wallet_sdk.locks_api();
        let num_cleared = transaction_api.clear_stale_locks()?;
        if num_cleared > 0 {
            info!(
                target: LOG_TARGET,
                "Cleared {} stale wallet lock(s)",
                num_cleared
            );
        } else {
            debug!(
                target: LOG_TARGET,
                "No stale wallet locks to clear",
            );
        }
        Ok(())
    }

    async fn resubmit_new_transactions(
        wallet_sdk: &WalletSdk<TSpec>,
        notify: &Notify<WalletEvent>,
    ) -> Result<(), TransactionServiceError> {
        let transaction_api = wallet_sdk.transaction_api();
        let new_transactions = transaction_api.fetch_all(Some(TransactionStatus::New), None)?;
        let log_level = if new_transactions.is_empty() {
            Level::Debug
        } else {
            Level::Info
        };
        log!(
            target: LOG_TARGET,
            log_level,
            "{} new transaction(s)",
            new_transactions.len()
        );
        for transaction in new_transactions {
            info!(
                target: LOG_TARGET,
                "Resubmitting transaction {}",
                transaction.id,
            );
            let transaction_id = transaction.id;
            if transaction_api.submit_transaction(transaction_id).await? {
                // Only NewAccountData is persisted in the DB, so only that context is recoverable on
                // resubmit. Other context variants (e.g. ClaimBurn) are in-memory only — if the daemon
                // restarts, that context is lost and the caller must retry.
                notify.notify(TransactionSubmittedEvent {
                    transaction_id,
                    context: transaction
                        .new_account_info
                        .map(|data| TransactionContext::default().with_kind(TransactionContextKind::NewAccount(data))),
                });
            } else {
                notify.notify(TransactionInvalidEvent {
                    transaction_id,
                    status: TransactionStatus::InvalidTransaction,
                    finalize: None,
                    final_fee: None,
                });
            }
        }
        Ok(())
    }

    async fn check_pending_transactions(
        wallet_sdk: &WalletSdk<TSpec>,
        notify: &Notify<WalletEvent>,
    ) -> Result<(), TransactionServiceError> {
        let transaction_api = wallet_sdk.transaction_api();
        let pending_transactions = transaction_api.fetch_all(Some(TransactionStatus::Pending), None)?;
        let log_level = if pending_transactions.is_empty() {
            Level::Debug
        } else {
            Level::Info
        };
        log!(
            target: LOG_TARGET,
            log_level,
            "{} pending transaction(s)",
            pending_transactions.len()
        );
        for transaction in pending_transactions {
            let tx_id = transaction.id;
            info!(
                target: LOG_TARGET,
                "Requesting result for transaction {tx_id}",
            );
            let maybe_finalized_transaction = transaction_api.check_and_store_finalized_transaction(tx_id).await?;

            match maybe_finalized_transaction {
                Some(transaction) => {
                    debug!(
                        target: LOG_TARGET,
                        "Transaction {} has been finalized: {}",
                        transaction.id,
                        transaction.status,
                    );
                    match transaction.finalize {
                        Some(finalize) => {
                            notify.notify(TransactionFinalizedEvent {
                                transaction_id: tx_id,
                                finalize,
                                final_fee: transaction.final_fee.unwrap_or_default(),
                                status: transaction.status,
                            });
                        },
                        None => notify.notify(TransactionInvalidEvent {
                            transaction_id: tx_id,
                            status: transaction.status,
                            finalize: transaction.finalize,
                            final_fee: transaction.final_fee,
                        }),
                    }
                },
                None => {
                    debug!(
                        target: LOG_TARGET,
                        "Transaction {tx_id} is still pending",
                    );
                },
            }
        }
        Ok(())
    }

    fn on_event(&mut self, event: WalletEvent) -> Result<(), TransactionServiceError> {
        match event {
            WalletEvent::TransactionSubmitted(_) => {
                let _ = self.trigger_poll.send(());
            },
            WalletEvent::TransactionInvalid(_) |
            WalletEvent::TransactionFinalized(_) |
            WalletEvent::AccountChangedOnChain(_) |
            WalletEvent::AuthLoginRequest(_) |
            WalletEvent::AccountCreatedOnChain(_) |
            WalletEvent::UtxoRecoveryStarted(_) |
            WalletEvent::UtxoRecovered(_) |
            WalletEvent::UtxoRecoveryCompleted(_) |
            WalletEvent::UtxoSpent(_) => {},
        }
        Ok(())
    }
}
