//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{collections::HashSet, fmt::Display};

use libp2p::{PeerId, gossipsub};
use log::*;
use tari_consensus::hotstuff::HotstuffEvent;
use tari_epoch_manager::{EpochManagerReader, service::EpochManagerHandle};
use tari_networking::NetworkingHandle;
use tari_ootle_common_types::optional::Optional;
use tari_ootle_p2p::{NewTransactionMessage, PeerAddress, TariMessage, TariMessagingSpec};
use tari_ootle_storage::{StateStore, StateStoreReadTransaction, consensus_models::TransactionRecord};
use tari_ootle_transaction::{Transaction, TransactionId};
use tokio::sync::{broadcast, mpsc, oneshot};

use super::MempoolError;
#[cfg(feature = "metrics")]
use super::metrics::PrometheusMempoolMetrics;
use crate::{
    consensus::ConsensusHandle,
    p2p::services::mempool::{
        gossip::{IncomingMessage, MempoolGossip},
        handle::MempoolRequest,
    },
    transaction_validators::TransactionValidationError,
    validator::Validator,
};

const LOG_TARGET: &str = "tari::validator_node::mempool::service";

const MEM_MAX_TRANSACTIONS_DEDUP_ALLOC: usize = 1_000_000; // 32Mb

#[derive(Debug)]
pub struct MempoolService<TValidator, TStateStore> {
    transactions: HashSet<TransactionId>,
    mempool_requests: mpsc::Receiver<MempoolRequest>,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    before_execute_validator: TValidator,
    state_store: TStateStore,
    gossip: MempoolGossip,
    consensus_handle: ConsensusHandle,
    #[cfg(feature = "metrics")]
    metrics: PrometheusMempoolMetrics,
}

impl<TValidator, TStateStore> MempoolService<TValidator, TStateStore>
where
    TValidator: Validator<Transaction, Context = (), Error = TransactionValidationError>,
    TStateStore: StateStore,
{
    pub(super) fn new(
        mempool_requests: mpsc::Receiver<MempoolRequest>,
        epoch_manager: EpochManagerHandle<PeerAddress>,
        before_execute_validator: TValidator,
        state_store: TStateStore,
        consensus_handle: ConsensusHandle,
        networking: NetworkingHandle<TariMessagingSpec>,
        rx_gossip: mpsc::UnboundedReceiver<(PeerId, gossipsub::Message)>,
        #[cfg(feature = "metrics")] metrics: PrometheusMempoolMetrics,
    ) -> Self {
        Self {
            gossip: MempoolGossip::new(networking, rx_gossip),
            transactions: Default::default(),
            mempool_requests,
            epoch_manager,
            before_execute_validator,
            state_store,
            consensus_handle,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut consensus_events = self.consensus_handle.subscribe_to_hotstuff_events()?;

        loop {
            tokio::select! {
                req = self.mempool_requests.recv() => {
                    match req {
                        Some(req) => self.handle_request(req).await,
                        None => {
                            info!(target: LOG_TARGET, "Mempool request channel closed, shutting down");
                            break;
                        }
                    }
                },
                result = self.gossip.next_message() => {
                    match result {
                        Some(msg) => {
                            if let Err(e) = self.handle_new_transaction_from_remote(msg).await {
                                warn!(target: LOG_TARGET, "Mempool rejected transaction: {}", e);
                            }
                        }
                        None => {
                            info!(target: LOG_TARGET, "Gossip channel closed, shutting down mempool service");
                            break;
                        }
                    };
                }
                event = consensus_events.recv() => {
                    match event {
                        Ok(HotstuffEvent::EpochChanged { epoch, registered_shard_group})  => {
                            if registered_shard_group.is_some() {
                                info!(target: LOG_TARGET, "Mempool service subscribing to transaction gossip in {epoch}");
                                self.gossip.subscribe().await?;
                            } else {
                                info!(target: LOG_TARGET, "Not registered for epoch {epoch}, unsubscribing from gossip if necessary");
                                self.gossip.unsubscribe().await?;
                            }
                        },
                        Ok(_) => {},
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(target: LOG_TARGET, "Missed {} consensus events", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!(target: LOG_TARGET, "Consensus event channel closed, shutting down mempool service");
                            break;
                        }
                    }
                },

                else => {
                    break;
                }
            }
        }

        self.gossip.unsubscribe().await?;

        info!(target: LOG_TARGET, "💤 Mempool service shutting down");
        Ok(())
    }

    async fn handle_request(&mut self, request: MempoolRequest) {
        match request {
            MempoolRequest::SubmitTransaction { transaction, reply } => {
                handle(reply, self.handle_new_transaction_from_local(*transaction).await);
            },
            MempoolRequest::RemoveTransactions { transaction_ids, reply } => {
                let num_found = self.remove_transactions(&transaction_ids);
                handle::<_, MempoolError>(reply, Ok(num_found));
            },
            MempoolRequest::GetMempoolSize { reply } => {
                let _ignore = reply.send(self.transactions.len());
            },
        }
    }

    fn remove_transactions(&mut self, ids: &[TransactionId]) -> usize {
        let mut num_found = 0;
        for id in ids {
            if self.transactions.remove(id) {
                num_found += 1;
            }
        }
        if self.transactions.capacity() > MEM_MAX_TRANSACTIONS_DEDUP_ALLOC {
            self.transactions.shrink_to(MEM_MAX_TRANSACTIONS_DEDUP_ALLOC);
        }
        num_found
    }

    async fn handle_new_transaction_from_local(&mut self, transaction: Transaction) -> Result<(), MempoolError> {
        if self.transaction_exists(&transaction.calculate_id())? {
            return Ok(());
        }
        info!(
            target: LOG_TARGET,
            "🎱 Received NEW transaction from local: {transaction}",
        );

        self.handle_new_transaction(transaction, true, self.gossip.get_num_incoming_messages())
            .await?;

        Ok(())
    }

    async fn handle_new_transaction_from_remote(
        &mut self,
        result: Result<IncomingMessage, MempoolError>,
    ) -> Result<(), MempoolError> {
        let IncomingMessage {
            address: from,
            message: msg,
            num_pending,
            message_size,
        } = result?;
        let TariMessage::NewTransaction(msg) = msg;
        let NewTransactionMessage { transaction } = *msg;
        let transaction_id = transaction.calculate_id();

        if !self.consensus_handle.is_running() {
            info!(
                target: LOG_TARGET,
                "🎱 Transaction {transaction_id} received while not in running state. Ignoring",
            );
            return Ok(());
        }

        if self.transaction_exists(&transaction_id)? {
            return Ok(());
        }
        debug!(
            target: LOG_TARGET,
            "Received NEW transaction from {}: (size={}) {} {:?}",
            from,
            message_size,
            transaction_id,
            transaction
        );

        self.handle_new_transaction(transaction, false, num_pending).await?;

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn handle_new_transaction(
        &mut self,
        transaction: Transaction,
        is_local: bool,
        num_pending: usize,
    ) -> Result<(), MempoolError> {
        #[cfg(feature = "metrics")]
        self.metrics.on_transaction_received(&transaction);
        let tx_id = transaction.calculate_id();

        if let Err(e) = self.before_execute_validator.validate(&(), &transaction) {
            // Throw the transaction away
            #[cfg(feature = "metrics")]
            self.metrics.on_transaction_validation_error(&tx_id, &e);
            return Err(e.into());
        }

        let current_epoch = self.consensus_handle.current_view().get_epoch();

        let local_committee_shard = self.epoch_manager.get_local_committee_info(current_epoch).await?;
        let is_involved = transaction.is_involved(&local_committee_shard);

        if is_involved {
            debug!(target: LOG_TARGET, "🎱 New transaction {tx_id} in mempool");
            self.transactions.insert(tx_id);
            self.consensus_handle
                .notify_new_transaction(transaction.clone(), num_pending)
                .await
                .map_err(|_| MempoolError::ConsensusChannelClosed)?;
        } else {
            debug!(
                target: LOG_TARGET,
                "🙇 Not in committee for transaction {tx_id}",
            );
        }

        // Transactions are gossiped on a single network-wide topic, so a single publish reaches every validator
        // (including all involved shard groups). Only the node that first introduces the transaction (received from a
        // local client) needs to publish it; transactions received from gossip are already seen by the whole network,
        // so re-publishing them would only produce Duplicate errors.
        if is_local {
            debug!(
                target: LOG_TARGET,
                "🎱 Propagating transaction {} ({} input(s))",
                tx_id,
                transaction.num_inputs(),
            );
            if let Err(e) = self.gossip.forward(NewTransactionMessage { transaction }).await {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ Failed to propagate transaction {tx_id}: {}",
                    e
                );
            }
        }

        Ok(())
    }

    fn transaction_exists(&self, id: &TransactionId) -> Result<bool, MempoolError> {
        if self.transactions.contains(id) {
            debug!(
                target: LOG_TARGET,
                "🎱 Transaction {} already in mempool",
                id
            );
            return Ok(true);
        }

        let transaction_exists = self.state_store.with_read_tx(|tx| {
            if tx
                .finalized_transaction_execution_get_finalized_time(id)
                .optional()?
                .is_some()
            {
                debug!(
                    target: LOG_TARGET,
                    "🎱 Transaction {} already finalized. Ignoring",
                    id
                );
                return Ok(true);
            }
            TransactionRecord::exists(tx, id)
        })?;

        if transaction_exists {
            debug!(
                target: LOG_TARGET,
                "🎱 Transaction {} already exists. Ignoring",
                id
            );
            return Ok(true);
        }

        Ok(false)
    }
}

fn handle<T, E: Display>(reply: oneshot::Sender<Result<T, E>>, result: Result<T, E>) {
    if let Err(ref e) = result {
        error!(target: LOG_TARGET, "Request failed with error: {}", e);
    }
    if reply.send(result).is_err() {
        error!(target: LOG_TARGET, "Requester abandoned request");
    }
}
