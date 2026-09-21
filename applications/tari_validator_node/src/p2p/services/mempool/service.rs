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

use std::{collections::HashSet, fmt::Display, mem, sync::Arc};

use libp2p::gossipsub::MessageAcceptance;
use log::*;
use tari_consensus::hotstuff::HotstuffEvent;
use tari_epoch_manager::{EpochManagerReader, service::EpochManagerHandle};
use tari_networking::{GossipMessage, NetworkingHandle};
use tari_ootle_common_types::{Epoch, optional::Optional};
use tari_ootle_p2p::{GossipValidation, NewTransactionMessage, PeerAddress, TariMessage, TariMessagingSpec};
use tari_ootle_storage::{StateStore, StateStoreReadTransaction, StorageError, consensus_models::TransactionRecord};
use tari_ootle_transaction::{Transaction, TransactionId};
use tari_ootle_transaction_validation::{TransactionValidationError, Validator};
use tokio::sync::{Semaphore, broadcast, mpsc, oneshot};

use super::MempoolError;
#[cfg(feature = "metrics")]
use super::metrics::PrometheusMempoolMetrics;
use crate::{
    consensus::ConsensusHandle,
    p2p::services::mempool::{
        gossip::{IncomingMessage, MempoolGossip},
        handle::MempoolRequest,
    },
    template_prewarm::TemplatePrewarmer,
};

const LOG_TARGET: &str = "tari::validator_node::mempool::service";

/// Admissions that may be waiting on a prewarm at one time.
///
/// Each holds its transaction until its wait ends, so this is what bounds the memory a burst of cold
/// templates can tie up, and the tasks it can create. Past it a transaction is handed to consensus
/// without waiting, which is where a node with no prewarm pool starts.
const MAX_CONCURRENT_PREWARM_WAITS: usize = 64;

/// Transaction ids the mempool remembers having seen. See [`SeenTransactions`] for the footprint
/// this implies; it is a cache with a database fallback, so this trades memory against how often a
/// re-gossiped transaction costs a lookup, not against correctness.
pub const MEM_MAX_TRANSACTIONS_DEDUP: usize = 1_000_000;

#[derive(Debug)]
pub struct MempoolService<TValidator, TStateStore> {
    transactions: SeenTransactions,
    mempool_requests: mpsc::Receiver<MempoolRequest>,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    before_execute_validator: TValidator,
    state_store: TStateStore,
    gossip: MempoolGossip,
    consensus_handle: ConsensusHandle,
    template_prewarmer: TemplatePrewarmer,
    prewarm_waits: Arc<Semaphore>,
    #[cfg(feature = "metrics")]
    metrics: PrometheusMempoolMetrics,
}

impl<TValidator, TStateStore> MempoolService<TValidator, TStateStore>
where
    TValidator: Validator<Transaction, Context = Epoch, Error = TransactionValidationError>,
    TStateStore: StateStore,
{
    pub(super) fn new(
        mempool_requests: mpsc::Receiver<MempoolRequest>,
        epoch_manager: EpochManagerHandle<PeerAddress>,
        before_execute_validator: TValidator,
        state_store: TStateStore,
        consensus_handle: ConsensusHandle,
        networking: NetworkingHandle<TariMessagingSpec>,
        rx_gossip: mpsc::Receiver<GossipMessage>,
        template_prewarmer: TemplatePrewarmer,
        #[cfg(feature = "metrics")] metrics: PrometheusMempoolMetrics,
    ) -> Self {
        Self {
            gossip: MempoolGossip::new(networking, rx_gossip),
            transactions: SeenTransactions::new(MEM_MAX_TRANSACTIONS_DEDUP),
            mempool_requests,
            epoch_manager,
            before_execute_validator,
            state_store,
            consensus_handle,
            template_prewarmer,
            prewarm_waits: Arc::new(Semaphore::new(MAX_CONCURRENT_PREWARM_WAITS)),
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
        num_found
    }

    async fn handle_new_transaction_from_local(&mut self, transaction: Transaction) -> Result<(), MempoolError> {
        let transaction_id = transaction.calculate_id();
        if self.transaction_exists(&transaction_id)? {
            return Ok(());
        }
        info!(
            target: LOG_TARGET,
            "🎱 Received NEW transaction from local: {transaction}",
        );

        self.handle_new_transaction(
            transaction,
            transaction_id,
            None,
            self.gossip.get_num_incoming_messages(),
        )
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
            validation,
        } = result?;
        let TariMessage::NewTransaction(msg) = msg;
        let NewTransactionMessage { transaction } = *msg;
        let transaction_id = transaction.calculate_id();

        // Well-formed but of no interest to us: withhold it without penalising the sender. Another
        // node still holding it will propagate it if it is useful to them.
        if !self.consensus_handle.is_running() {
            info!(
                target: LOG_TARGET,
                "🎱 Transaction {transaction_id} received while not in running state. Ignoring",
            );
            self.gossip.report(validation, MessageAcceptance::Ignore).await;
            return Ok(());
        }

        if self.transaction_exists(&transaction_id)? {
            self.gossip.report(validation, MessageAcceptance::Ignore).await;
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

        self.handle_new_transaction(transaction, transaction_id, Some(validation), num_pending)
            .await?;

        Ok(())
    }

    /// `tx_id` must be the id of `transaction`. Taken from the caller rather than recomputed:
    /// deriving it hashes every blob payload, and both callers already hold it.
    ///
    /// `gossip_validation` is `Some` for a transaction that arrived over gossip, and carries the
    /// handle its verdict must be reported against; `None` marks a transaction introduced by a local
    /// client, which we are responsible for publishing.
    #[allow(clippy::too_many_lines)]
    async fn handle_new_transaction(
        &mut self,
        transaction: Transaction,
        tx_id: TransactionId,
        gossip_validation: Option<GossipValidation>,
        num_pending: usize,
    ) -> Result<(), MempoolError> {
        #[cfg(feature = "metrics")]
        self.metrics.on_transaction_received(&transaction);
        let is_local = gossip_validation.is_none();

        // The epoch-dependent rules are checked here, alongside the structural ones, so that a
        // transaction outside its validity window is refused before it is admitted or re-gossiped
        // rather than after. Both feed the single acceptance verdict below.
        let current_epoch = self.consensus_handle.current_view().get_epoch();
        let validation_result = self.before_execute_validator.validate(&current_epoch, &transaction);

        // Reported here rather than at the end of this function: everything below is about whether
        // *we* act on the transaction, not whether it is valid, and gossipsub only holds a message
        // in its validation cache for a few heartbeats. A transaction we are not involved in is
        // still accepted — other shard groups need it.
        if let Some(validation) = gossip_validation {
            // Only a failure the sender is responsible for is rejected back to the mesh, since
            // rejection counts against their peer score. A transaction we consider invalid because
            // of our own view of runtime state — a template we have not synced — or because of our
            // own storage health is withheld without penalty: a peer that is ahead of us, or our
            // own failing database, must not graylist honest peers.
            let acceptance = match &validation_result {
                Ok(_) => MessageAcceptance::Accept,
                Err(err) if err.is_sender_fault() => MessageAcceptance::Reject,
                Err(_) => MessageAcceptance::Ignore,
            };
            self.gossip.report(validation, acceptance).await;
        }

        if let Err(e) = validation_result {
            // Throw the transaction away
            #[cfg(feature = "metrics")]
            self.metrics.on_transaction_validation_error(&tx_id, &e);
            return Err(e.into());
        }

        let local_committee_shard = self.epoch_manager.get_local_committee_info(current_epoch).await?;
        let is_involved = transaction.is_involved(&local_committee_shard);

        if !is_involved {
            debug!(
                target: LOG_TARGET,
                "🙇 Not in committee for transaction {tx_id}",
            );
            if is_local {
                self.propagate(tx_id, transaction).await;
            }
            return Ok(());
        }

        debug!(target: LOG_TARGET, "🎱 New transaction {tx_id} in mempool");
        self.transactions.insert(tx_id);

        // Propagated before the prewarm below, so that every other involved validator starts its own
        // compile at the same moment this one does. Holding it until this node is warm would serialise
        // what is meant to happen network-wide in parallel.
        if is_local {
            self.propagate(tx_id, transaction.clone()).await;
        }

        // Validated and ours to execute, which is what makes this compile work the node is going to
        // do anyway rather than work anyone who can gossip can ask it for.
        //
        // Consensus is told about the transaction only once the compile is done or the wait expires.
        // A leader executes inline while building a proposal, so a transaction handed over cold is a
        // Cranelift compile inside the proposal path; withholding it until this node is warm keeps any
        // proposal it appears in a warm one. The bound is what keeps that a latency decision rather
        // than a liveness one: on timeout, on a full queue, or on a failed compile the transaction is
        // handed over regardless, and the compile it was waiting on runs on for the executor to join.
        let wait = self.template_prewarmer.prewarm_transaction(&transaction);

        // A transaction with nothing to wait for is handed over on this task, which keeps the common
        // case in arrival order and keeps this function's error path intact.
        if wait.is_empty() {
            return self.notify_consensus(transaction, num_pending).await;
        }

        // Anything that does wait, waits on a task of its own. This service is a single task serving
        // gossip, mempool requests and consensus events from one `select!`, and its inbound gossip
        // queue drops on overflow rather than pushing back, so a wait taken here would be paid by
        // every message queued behind it and would turn a burst of cold templates into lost
        // transactions. The permit bounds how many transactions can be held this way at once; without
        // one the transaction is handed over immediately, as it is when the prewarm queue is full.
        let Ok(permit) = self.prewarm_waits.clone().try_acquire_owned() else {
            debug!(
                target: LOG_TARGET,
                "🎱 Handing transaction {tx_id} to consensus without waiting: too many admissions are already waiting",
            );
            return self.notify_consensus(transaction, num_pending).await;
        };

        let consensus_handle = self.consensus_handle.clone();
        tokio::spawn(async move {
            let _permit = permit;
            wait.wait().await;
            if let Err(e) = consensus_handle.notify_new_transaction(transaction, num_pending).await {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ Failed to hand transaction {tx_id} to consensus after its prewarm: {e}",
                );
            }
        });

        Ok(())
    }

    /// Hand a transaction this node is involved in to consensus.
    ///
    /// Ordering between transactions is not preserved once one of them waits for a prewarm, and does
    /// not need to be: consensus holds its pool as a set and a proposal orders from it by its own
    /// rules, so what arrival order decides here is which transaction a leader sees first, not what
    /// any block contains.
    async fn notify_consensus(&self, transaction: Transaction, num_pending: usize) -> Result<(), MempoolError> {
        self.consensus_handle
            .notify_new_transaction(transaction, num_pending)
            .await
            .map_err(|_| MempoolError::ConsensusChannelClosed)
    }

    /// Publish a transaction this node introduced to the network.
    ///
    /// Transactions are gossiped on a single network-wide topic, so a single publish reaches every
    /// validator, including every involved shard group. Only the node a transaction was submitted to
    /// publishes it; one received from gossip is already seen by the whole network, and re-publishing
    /// it would only produce Duplicate errors.
    async fn propagate(&mut self, tx_id: TransactionId, transaction: Transaction) {
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
            if TransactionRecord::exists(tx, id)? {
                return Ok(true);
            }
            // A commit outlives the local records: the transaction's receipt substate proves the id
            // has committed even when the records were pruned. Best-effort — a node that does not
            // host the receipt's shard (or is behind on sync) simply never finds it and the
            // consensus gate makes the authoritative refusal.
            if TransactionRecord::receipt_exists(tx, id)? {
                debug!(
                    target: LOG_TARGET,
                    "🎱 Transaction {} has a receipt in state and has already committed. Ignoring",
                    id
                );
                return Ok(true);
            }
            Ok::<_, StorageError>(false)
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

/// Bounded cache of transaction ids the mempool has already seen.
///
/// Purely a fast path: [`MempoolService::transaction_exists`] falls through to the state store on a
/// miss, so forgetting an id costs one extra database read should that transaction arrive again,
/// and nothing more. That is what makes a hard bound safe here — without one the cache grows with
/// the unfinalized backlog, which nothing else bounds. It is also what makes coarse eviction
/// acceptable, which is what keeps the footprint down.
///
/// Ids are held in two generations rather than a set alongside an eviction queue: a queue would
/// store every id a second time, and exact eviction order is worth less than that memory when a
/// miss is merely a database read. Inserts land in the current generation; when it fills, it
/// becomes the previous generation and the older one is dropped wholesale. Lookups check both, so
/// an id is remembered for between `capacity / 2` and `capacity` subsequent inserts.
///
/// Footprint is roughly `capacity` × 33 bytes across the two tables, rounded up to whatever
/// power-of-two bucket count each needs — so tens of MiB at the capacity used here, against
/// roughly double that for a set plus a queue.
#[derive(Debug)]
struct SeenTransactions {
    current: HashSet<TransactionId>,
    previous: HashSet<TransactionId>,
    generation_capacity: usize,
}

impl SeenTransactions {
    fn new(capacity: usize) -> Self {
        Self {
            current: HashSet::new(),
            previous: HashSet::new(),
            generation_capacity: capacity.div_ceil(2).max(1),
        }
    }

    fn contains(&self, id: &TransactionId) -> bool {
        self.current.contains(id) || self.previous.contains(id)
    }

    fn insert(&mut self, id: TransactionId) {
        if self.contains(&id) {
            return;
        }
        if self.current.len() >= self.generation_capacity {
            self.previous = mem::take(&mut self.current);
        }
        self.current.insert(id);
    }

    fn remove(&mut self, id: &TransactionId) -> bool {
        // Which generation holds the id is not tracked, so both are cleared.
        let was_current = self.current.remove(id);
        let was_previous = self.previous.remove(id);
        was_current || was_previous
    }

    fn len(&self) -> usize {
        self.current.len() + self.previous.len()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> TransactionId {
        TransactionId::new([n; 32])
    }

    #[test]
    fn evicts_oldest_ids_beyond_capacity() {
        let mut seen = SeenTransactions::new(2);
        seen.insert(id(1));
        seen.insert(id(2));
        seen.insert(id(3));

        assert!(!seen.contains(&id(1)), "the oldest id is evicted");
        assert!(seen.contains(&id(2)));
        assert!(seen.contains(&id(3)));
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn reinserting_a_known_id_does_not_consume_capacity() {
        let mut seen = SeenTransactions::new(4);
        for _ in 0..10 {
            seen.insert(id(1));
        }
        assert_eq!(seen.len(), 1);
        assert!(seen.previous.is_empty(), "duplicates must not drive a rotation");
    }

    #[test]
    fn removed_ids_are_forgotten_from_either_generation() {
        let mut seen = SeenTransactions::new(2);
        seen.insert(id(1));
        seen.insert(id(2));
        // One id per generation at this capacity, so these are now in different generations.
        assert!(seen.remove(&id(1)));
        assert!(seen.remove(&id(2)));
        assert!(!seen.contains(&id(1)));
        assert!(!seen.contains(&id(2)));
        assert_eq!(seen.len(), 0);
        assert!(!seen.remove(&id(1)), "removing an unknown id reports nothing found");
    }

    #[test]
    fn total_retained_never_exceeds_capacity() {
        let mut seen = SeenTransactions::new(8);
        for n in 0..=255u8 {
            seen.insert(id(n));
            assert!(seen.len() <= 8, "cache grew past its bound at id {n}");
        }
        assert!(seen.contains(&id(255)), "the most recent id is always retained");
    }
}
