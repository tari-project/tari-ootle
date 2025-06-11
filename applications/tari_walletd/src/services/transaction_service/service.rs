//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{sync::Arc, time::Duration};

use log::*;
use tari_engine_types::commit_result::ExecuteResult;
use tari_ootle_common_types::optional::IsNotFoundError;
use tari_ootle_wallet_sdk::{
    models::{NewAccountInfo, TransactionStatus},
    network::WalletNetworkInterface,
    storage::WalletStore,
    WalletSdk,
};
use tari_shutdown::ShutdownSignal;
use tari_transaction::{Transaction, TransactionId};
use tokio::{
    sync::{mpsc, watch, Semaphore},
    time,
    time::MissedTickBehavior,
};

use super::{
    error::TransactionServiceError,
    handle::{TransactionServiceHandle, TransactionServiceRequest},
};
use crate::{
    notify::Notify,
    services::{TransactionFinalizedEvent, TransactionInvalidEvent, TransactionSubmittedEvent, WalletEvent},
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::transaction_service";

pub struct TransactionService<TStore, TNetworkInterface> {
    rx_request: mpsc::Receiver<TransactionServiceRequest>,
    notify: Notify<WalletEvent>,
    wallet_sdk: WalletSdk<TStore, TNetworkInterface>,
    trigger_poll: watch::Sender<()>,
    rx_trigger: watch::Receiver<()>,
    poll_semaphore: Arc<Semaphore>,
    shutdown_signal: ShutdownSignal,
}

impl<TStore, TNetworkInterface> TransactionService<TStore, TNetworkInterface>
where
    TStore: WalletStore + Clone + Send + Sync + 'static,
    TNetworkInterface: WalletNetworkInterface + Clone + Send + Sync + 'static,
    TNetworkInterface::Error: IsNotFoundError,
{
    pub fn new(
        notify: Notify<WalletEvent>,
        wallet_sdk: WalletSdk<TStore, TNetworkInterface>,
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
                new_account_info,
                reply,
            } => {
                reply
                    .send(self.handle_submit_transaction(transaction, new_account_info).await)
                    .map_err(|_| TransactionServiceError::ServiceShutdown)?;
            },
            TransactionServiceRequest::SubmitDryRunTransaction { transaction, reply } => {
                let transaction_id = transaction.calculate_id();
                let transaction_api = self.wallet_sdk.transaction_api();
                match transaction_api.submit_dry_run_transaction(transaction).await {
                    Ok(finalized_transaction) => {
                        // Unlock all proofs related to the transaction
                        transaction_api.release_all_outputs_for_transaction(transaction_id)?;

                        let finalize = finalized_transaction.finalize.ok_or_else(|| {
                            TransactionServiceError::DryRunTransactionFailed {
                                details: "Transaction was not finalized".to_string(),
                            }
                        });
                        reply
                            .send(finalize.map(|finalize| ExecuteResult {
                                finalize,
                                execution_time: finalized_transaction.execution_time.unwrap_or_default(),
                            }))
                            .map_err(|_| TransactionServiceError::ServiceShutdown)?;
                    },
                    Err(e) => {
                        if let Err(err) = transaction_api.release_all_outputs_for_transaction(transaction_id) {
                            error!(target: LOG_TARGET, "Error releasing outputs for transaction {}: {}", transaction_id, err);
                        }

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
        new_account_info: Option<NewAccountInfo>,
    ) -> Result<TransactionId, TransactionServiceError> {
        let transaction_api = self.wallet_sdk.transaction_api();
        let transaction_id = transaction_api
            .insert_new_transaction(transaction, vec![], new_account_info.clone(), false)
            .await?;
        transaction_api.submit_transaction(transaction_id).await?;
        self.notify.notify(TransactionSubmittedEvent {
            transaction_id,
            new_account: new_account_info,
        });
        Ok(transaction_id)
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
                error!(target: LOG_TARGET, "Error re-submitting new transactions: {}", err);
            }
            if let Err(err) = Self::check_pending_transactions(&wallet_sdk, &notify).await {
                error!(target: LOG_TARGET, "Error checking pending transactions: {}", err);
            }

            drop(permit);
        });
        Ok(())
    }

    async fn resubmit_new_transactions(
        wallet_sdk: &WalletSdk<TStore, TNetworkInterface>,
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
            transaction_api.submit_transaction(transaction_id).await?;
            notify.notify(TransactionSubmittedEvent {
                transaction_id,
                new_account: transaction.new_account_info,
            });
        }
        Ok(())
    }

    async fn check_pending_transactions(
        wallet_sdk: &WalletSdk<TStore, TNetworkInterface>,
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
            WalletEvent::AccountChanged(_) |
            WalletEvent::AuthLoginRequest(_) |
            WalletEvent::AccountCreated(_) => {},
        }
        Ok(())
    }
}
