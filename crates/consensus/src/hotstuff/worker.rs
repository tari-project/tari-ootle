//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::{Debug, Formatter},
    iter,
};

use log::*;
use tari_common_types::types::FixedHash;
use tari_consensus_types::{
    HighPc,
    HighTc,
    HighestSeenBlock,
    LastProposed,
    ProposalCertificate,
    QcId,
    TimeoutCertificate,
};
use tari_epoch_manager::{EpochManagerEvent, EpochManagerReader};
use tari_ootle_common_types::{optional::Optional, Epoch, NodeHeight, ShardGroup};
use tari_ootle_storage::{
    consensus_models::{
        Block,
        BookkeepingModel,
        BurntUtxo,
        ForeignProposalRecord,
        NoVoteReason,
        TransactionPool,
        TransactionRecord,
    },
    StateStore,
};
use tari_shutdown::ShutdownSignal;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tari_transaction::{Transaction, TransactionId};
use tokio::sync::{broadcast, mpsc};

use super::{
    calculate_last_dummy_block,
    config::HotstuffConfig,
    get_highest_seen_justified_view,
    on_receive_new_transaction::OnReceiveNewTransaction,
    ProposalValidationError,
};
use crate::{
    hotstuff::{
        epoch_gc::EpochGc,
        epoch_state::EpochState,
        error::HotStuffError,
        event::HotstuffEvent,
        on_catch_up_sync::OnCatchUpSync,
        on_catch_up_sync_request::OnSyncRequest,
        on_inbound_message::OnInboundMessage,
        on_message_validate::{MessageValidationResult, OnMessageValidate},
        on_next_sync_view::OnNextSyncViewHandler,
        on_propose::OnPropose,
        on_receive_foreign_proposal::OnReceiveForeignProposalHandler,
        on_receive_local_proposal::OnReceiveLocalProposalHandler,
        on_receive_new_view::OnReceiveNewViewHandler,
        on_receive_request_missing_transactions::OnReceiveRequestMissingTransactions,
        on_receive_vote::OnReceiveVoteHandler,
        pacemaker::PaceMaker,
        pacemaker_handle::PaceMakerHandle,
        state_tree_gc::StateTreeGc,
        substate_store::ShardedStateTree,
        transaction_manager::ConsensusTransactionManager,
        vote_collector::{ProposalVoteCollector, TimeoutVoteCollector},
    },
    messages::{HotstuffMessage, ProposalMessage},
    tracing::TraceTimer,
    traits::{hooks::ConsensusHooks, CertificateStore, ConsensusSpec, LeaderStrategy, PeriodicTask},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::worker";

pub struct HotstuffWorker<TConsensusSpec: ConsensusSpec> {
    local_validator_addr: TConsensusSpec::Addr,

    config: HotstuffConfig,
    hooks: TConsensusSpec::Hooks,

    tx_events: broadcast::Sender<HotstuffEvent>,
    rx_new_transactions: mpsc::Receiver<(Transaction, usize)>,
    rx_missing_transactions: mpsc::UnboundedReceiver<Vec<TransactionId>>,

    on_inbound_message: OnInboundMessage<TConsensusSpec>,
    on_next_sync_view: OnNextSyncViewHandler<TConsensusSpec>,
    on_receive_local_proposal: OnReceiveLocalProposalHandler<TConsensusSpec>,
    on_receive_foreign_proposal: OnReceiveForeignProposalHandler<TConsensusSpec>,
    on_receive_vote: OnReceiveVoteHandler<TConsensusSpec>,
    on_receive_new_view: OnReceiveNewViewHandler<TConsensusSpec>,
    on_receive_request_missing_txs: OnReceiveRequestMissingTransactions<TConsensusSpec>,
    on_receive_new_transaction: OnReceiveNewTransaction<TConsensusSpec>,
    on_message_validate: OnMessageValidate<TConsensusSpec>,
    on_propose: OnPropose<TConsensusSpec>,
    on_sync_request: OnSyncRequest<TConsensusSpec>,
    on_catch_up_sync: OnCatchUpSync<TConsensusSpec>,

    state_store: TConsensusSpec::StateStore,
    leader_strategy: TConsensusSpec::LeaderStrategy,
    transaction_pool: TransactionPool<TConsensusSpec::StateStore>,

    epoch_manager: TConsensusSpec::EpochManager,
    pacemaker_worker: Option<PaceMaker>,
    pacemaker: PaceMakerHandle,
    shutdown: ShutdownSignal,
}
impl<TConsensusSpec: ConsensusSpec> HotstuffWorker<TConsensusSpec> {
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    pub fn new(
        config: HotstuffConfig,
        local_validator_addr: TConsensusSpec::Addr,
        inbound_messaging: TConsensusSpec::InboundMessaging,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
        rx_new_transactions: mpsc::Receiver<(Transaction, usize)>,
        state_store: TConsensusSpec::StateStore,
        epoch_manager: TConsensusSpec::EpochManager,
        leader_strategy: TConsensusSpec::LeaderStrategy,
        signing_service: TConsensusSpec::SignerService,
        transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
        transaction_executor: TConsensusSpec::TransactionExecutor,
        tx_events: broadcast::Sender<HotstuffEvent>,
        hooks: TConsensusSpec::Hooks,
        shutdown: ShutdownSignal,
    ) -> Self {
        let (tx_missing_transactions, rx_missing_transactions) = mpsc::unbounded_channel();
        let pacemaker = PaceMaker::new(config.consensus_constants.pacemaker_block_time);
        let proposal_vote_collector = ProposalVoteCollector::new(
            config.network,
            state_store.clone(),
            epoch_manager.clone(),
            signing_service.clone(),
        );
        let timeout_vote_collector = TimeoutVoteCollector::new(
            config.network,
            state_store.clone(),
            epoch_manager.clone(),
            signing_service.clone(),
        );
        let transaction_manager = ConsensusTransactionManager::new(transaction_executor.clone());

        Self {
            local_validator_addr: local_validator_addr.clone(),

            config: config.clone(),
            tx_events: tx_events.clone(),
            rx_new_transactions,
            rx_missing_transactions,

            on_inbound_message: OnInboundMessage::new(inbound_messaging, hooks.clone()),
            on_message_validate: OnMessageValidate::new(
                config.clone(),
                state_store.clone(),
                epoch_manager.clone(),
                pacemaker.clone_handle().current_view().clone(),
                leader_strategy.clone(),
                signing_service.clone(),
                outbound_messaging.clone(),
                tx_events.clone(),
            ),

            on_next_sync_view: OnNextSyncViewHandler::new(
                state_store.clone(),
                outbound_messaging.clone(),
                leader_strategy.clone(),
                signing_service.clone(),
            ),
            on_receive_local_proposal: OnReceiveLocalProposalHandler::new(
                state_store.clone(),
                epoch_manager.clone(),
                leader_strategy.clone(),
                pacemaker.clone_handle(),
                outbound_messaging.clone(),
                signing_service.clone(),
                transaction_pool.clone(),
                tx_events,
                transaction_manager.clone(),
                config.clone(),
                hooks.clone(),
            ),
            on_receive_foreign_proposal: OnReceiveForeignProposalHandler::new(
                state_store.clone(),
                epoch_manager.clone(),
                pacemaker.clone_handle(),
                outbound_messaging.clone(),
            ),
            on_receive_vote: OnReceiveVoteHandler::new(
                pacemaker.clone_handle(),
                proposal_vote_collector.clone(),
                local_validator_addr.clone(),
                leader_strategy.clone(),
            ),
            on_receive_new_view: OnReceiveNewViewHandler::new(
                local_validator_addr,
                state_store.clone(),
                leader_strategy.clone(),
                pacemaker.clone_handle(),
                proposal_vote_collector,
                timeout_vote_collector,
            ),
            on_receive_request_missing_txs: OnReceiveRequestMissingTransactions::new(
                state_store.clone(),
                outbound_messaging.clone(),
            ),
            on_receive_new_transaction: OnReceiveNewTransaction::new(
                state_store.clone(),
                transaction_pool.clone(),
                transaction_executor.clone(),
                tx_missing_transactions,
            ),
            on_propose: OnPropose::new(
                config,
                state_store.clone(),
                epoch_manager.clone(),
                transaction_pool.clone(),
                transaction_manager,
                signing_service,
                outbound_messaging.clone(),
            ),

            on_sync_request: OnSyncRequest::new(state_store.clone(), outbound_messaging.clone()),
            on_catch_up_sync: OnCatchUpSync::new(state_store.clone(), pacemaker.clone_handle(), outbound_messaging),

            state_store,
            leader_strategy,
            epoch_manager,
            transaction_pool,

            pacemaker: pacemaker.clone_handle(),
            pacemaker_worker: Some(pacemaker),
            hooks,
            shutdown,
        }
    }

    pub fn pacemaker(&self) -> &PaceMakerHandle {
        &self.pacemaker
    }

    pub async fn start(&mut self) -> Result<(), HotStuffError> {
        let current_epoch = self.epoch_manager.current_epoch().await?;
        let current_epoch_hash = self.epoch_manager.get_current_epoch_hash().await?;
        let local_committee_info = self.epoch_manager.get_local_committee_info(current_epoch).await?;

        self.create_genesis_block_if_required(current_epoch, current_epoch_hash, local_committee_info.shard_group())?;

        // Resume pacemaker from the last epoch/height
        let current_height = self
            .state_store
            .with_read_tx(|tx| get_highest_seen_justified_view(tx, current_epoch))?;

        info!(
            target: LOG_TARGET,
            "🚀 Pacemaker starting for epoch {}, height: {}",
            current_epoch,
            current_height,
        );

        self.pacemaker
            .start(current_epoch, current_height, current_height)
            .await?;
        self.publish_event(HotstuffEvent::EpochChanged {
            epoch: current_epoch,
            registered_shard_group: Some(local_committee_info.shard_group()),
        });

        let local_committee = self.epoch_manager.get_local_committee(current_epoch).await?;
        let epoch_state = EpochState {
            epoch: current_epoch,
            epoch_hash: current_epoch_hash,
            local_committee_info,
            local_committee,
        };

        let _cancel_state_tree_gc_task_on_drop = StateTreeGc::new(
            self.state_store.clone(),
            epoch_state.local_committee_info.num_preshards(),
        )
        .do_work_periodically(self.config.state_tree_cleanup_interval);

        let _cancel_epoch_gc_task_on_drop =
            EpochGc::new(self.state_store.clone()).do_work_periodically(self.config.epoch_gc_interval);

        self.run(epoch_state).await?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn run(&mut self, mut epoch_state: EpochState<TConsensusSpec::Addr>) -> Result<(), HotStuffError> {
        // Spawn pacemaker if not spawned already
        if let Some(pm) = self.pacemaker_worker.take() {
            pm.spawn();
        }

        let mut on_force_beat = self.pacemaker.get_on_force_beat();
        let mut on_leader_timeout = self.pacemaker.get_on_leader_timeout();

        let mut epoch_manager_events = self.epoch_manager.subscribe();

        let mut prev_height = self.pacemaker.current_view().get_height();
        let current_epoch = self.pacemaker.current_view().get_epoch();
        // self.request_initial_catch_up_sync(current_epoch).await?;
        let mut local_claim_public_key = self
            .epoch_manager
            .get_our_validator_node(current_epoch)
            .await?
            .fee_claim_public_key;

        self.request_initial_catch_up_sync(&epoch_state).await?;

        loop {
            let current_height = self.pacemaker.current_view().get_height();
            let current_epoch = self.pacemaker.current_view().get_epoch();

            // Need to update epoch state if the epoch has changed
            if epoch_state.epoch != current_epoch {
                epoch_state
                    .update_from_epoch_manager(&self.epoch_manager, current_epoch)
                    .await?;
                local_claim_public_key = self
                    .epoch_manager
                    .get_our_validator_node(current_epoch)
                    .await?
                    .fee_claim_public_key;
            }

            if current_height != prev_height {
                self.hooks.on_pacemaker_height_changed(current_height);
                prev_height = current_height;
            }

            debug!(
                target: LOG_TARGET,
                "🔥 {} Current height #{}",
                self.local_validator_addr,
                current_height.as_u64()
            );

            tokio::select! {
                Ok(event) = epoch_manager_events.recv() => {
                    self.on_epoch_manager_event(event).await?;
                },

                forced_height = on_force_beat.wait() => {
                    if let Err(e) = self.on_force_beat(&epoch_state, current_height, forced_height, &local_claim_public_key).await {
                        self.on_failure("propose_if_leader", &e).await;
                        return Err(e);
                    }
                },

                _ = self.pacemaker.on_beat() => {
                    if let Err(e) = self.on_beat(&epoch_state,  &local_claim_public_key).await {
                        self.on_failure("on_beat", &e).await;
                        return Err(e);
                    }
                },

                Some((tx_id, pending)) = self.rx_new_transactions.recv() => {
                    if let Err(err) = self.on_new_transaction(tx_id, pending, &epoch_state,current_height ).await {
                        self.hooks.on_error(&err);
                        error!(target: LOG_TARGET, "🚨Error handling new transaction: {}", err);
                    }
                },

                Some(result) = self.on_inbound_message.next_message(epoch_state.epoch(), current_height) => {
                    if let Err(e) = self.on_unvalidated_message(&epoch_state, current_height, result).await {
                        self.on_failure("on_unvalidated_message", &e).await;
                        return Err(e);
                    }
                },

               // TODO: This channel is used to work around some design-flaws in missing transactions handling.
                //       We cannot simply call check_if_block_can_be_unparked in dispatch_hotstuff_message as that creates a cycle.
                //       One suggestion is to refactor consensus to emit events (kinda like libp2p does) and handle those events.
                //       This should be easy to reason about and avoid a large depth of async calls and "callback channels".
                Some(batch) = self.rx_missing_transactions.recv() => {
                    if let Err(err) = self.check_if_block_can_be_unparked(&epoch_state, current_height, batch.iter()).await {
                        self.hooks.on_error(&err);
                        error!(target: LOG_TARGET, "🚨Error handling missing transaction: {}", err);
                    }
                },

                _ = on_leader_timeout.wait() => {
                    if let Err(e) = self.on_leader_timeout(&epoch_state, current_height).await {
                        self.on_failure("on_leader_timeout", &e).await;
                        return Err(e);
                    }
                },

                _ = self.shutdown.wait() => {
                    info!(target: LOG_TARGET, "💤 Consensus shutting down");
                    break;
                }
            }
        }

        self.on_inbound_message.clear_buffer();
        // This only happens if we're shutting down.
        if let Err(err) = self.pacemaker.stop().await {
            debug!(target: LOG_TARGET, "Pacemaker channel dropped: {}", err);
        }

        Ok(())
    }

    async fn on_unvalidated_message(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        current_height: NodeHeight,
        result: Result<(TConsensusSpec::Addr, HotstuffMessage), HotStuffError>,
    ) -> Result<(), HotStuffError> {
        let (from, msg) = result?;

        match self
            .on_message_validate
            .handle(current_height, epoch_state, from.clone(), msg)
            .await?
        {
            MessageValidationResult::Ready { from, message: msg } => {
                if let Err(e) = self
                    .dispatch_hotstuff_message(epoch_state, current_height, from, msg)
                    .await
                {
                    return self.handle_hotstuff_error(epoch_state, None, e).await;
                }
                Ok(())
            },
            MessageValidationResult::ParkedProposal {
                epoch,
                missing_txs,
                block_id,
                ..
            } => {
                let mut request_from_address = from;
                if request_from_address == self.local_validator_addr {
                    // Edge case: If we're catching up, we could be the proposer but we no longer have
                    // the transaction (we deleted our database likely during development testing).
                    // In this case, request from another random VN.
                    // (TODO: not 100% reliable since we're just asking a single random committee member)
                    let mut local_committee = self.epoch_manager.get_local_committee(epoch).await?;

                    local_committee.shuffle();
                    match local_committee
                        .into_iter()
                        .find(|m| m.address != self.local_validator_addr)
                    {
                        Some(m) => {
                            warn!(
                                target: LOG_TARGET,
                                "⚠️Requesting missing transactions from another validator {} because we are (presumably) catching up (local_peer_id = {})",
                                m,
                                self.local_validator_addr,
                            );
                            request_from_address = m.address;
                        },
                        None => {
                            warn!(
                                target: LOG_TARGET,
                                "❌NEVERHAPPEN: We're the only validator in the committee but we need to request missing transactions."
                            );
                            return Ok(());
                        },
                    }
                }

                self.on_message_validate
                    .request_missing_transactions(request_from_address, block_id, epoch, missing_txs)
                    .await?;
                Ok(())
            },
            MessageValidationResult::Discard => Ok(()),
            // In these cases, we want to propagate the error back to the state machine, to allow sync
            MessageValidationResult::Invalid { err, message, from } => {
                if let HotStuffError::ProposalValidationError(_) = err {
                    error!(target: LOG_TARGET, "🚨 Invalid message from {from}: {err} - {message}");
                }

                self.handle_hotstuff_error(epoch_state, None, err).await
            },
        }
    }

    async fn on_new_transaction(
        &mut self,
        transaction: Transaction,
        num_pending_txs: usize,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        current_height: NodeHeight,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::info(LOG_TARGET, "on_new_transaction");
        let maybe_transaction = self.on_receive_new_transaction.try_sequence_transaction(
            epoch_state.epoch(),
            TransactionRecord::new(transaction),
            epoch_state.local_committee_info(),
        )?;

        let Some(transaction) = maybe_transaction else {
            return Ok(());
        };

        info!(
            target: LOG_TARGET,
            "🔥 new transaction ready for consensus: {} ({} pending)",
            transaction.id(),
            num_pending_txs,
        );

        self.hooks.on_transaction_ready(transaction.id());

        if self
            .check_if_block_can_be_unparked(epoch_state, current_height, iter::once(transaction.id()))
            .await?
        {
            // No need to call on_beat, a block was unparked so on_beat will be called as needed
            return Ok(());
        }

        // There are num_pending_txs transactions in the queue. If we have no pending transactions, we'll propose now if
        // able.
        if num_pending_txs == 0 {
            self.pacemaker.beat();
        }

        Ok(())
    }

    /// Returns true if a block was unparked, otherwise false
    async fn check_if_block_can_be_unparked<
        'a,
        I: IntoIterator<Item = &'a TransactionId> + ExactSizeIterator + Clone,
    >(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        current_height: NodeHeight,
        transaction_ids: I,
    ) -> Result<bool, HotStuffError> {
        let (local_proposals, foreign_proposals) = self
            .on_message_validate
            .update_parked_blocks(current_height, transaction_ids)?;

        let is_any_block_unparked = !local_proposals.is_empty() || !foreign_proposals.is_empty();

        for msg in foreign_proposals {
            if let Err(e) = self
                .on_receive_foreign_proposal
                .handle_received(msg, epoch_state.local_committee_info())
                .await
            {
                self.on_failure("check_if_block_can_be_unparked -> on_receive_foreign_proposal", &e)
                    .await;
                return Err(e);
            }
        }

        for msg in local_proposals {
            if let Err(e) = self.on_proposal_message(epoch_state, current_height, msg).await {
                self.on_failure("check_if_block_can_be_unparked -> on_proposal_message", &e)
                    .await;
                return Err(e);
            }
        }

        Ok(is_any_block_unparked)
    }

    async fn on_epoch_manager_event(&mut self, event: EpochManagerEvent) -> Result<(), HotStuffError> {
        match event {
            EpochManagerEvent::EpochChanged {
                epoch,
                registered_shard_group,
            } => {
                if registered_shard_group.is_none() {
                    let current_epoch = self.pacemaker.current_view().get_epoch();
                    if current_epoch < epoch {
                        info!(
                            target: LOG_TARGET,
                            "💤 This validator is not registered for next epoch {epoch}. Will stop consensus once the current epoch {current_epoch} has transitioned."
                        );
                        return Ok(());
                    }
                    info!(
                        target: LOG_TARGET,
                        "💤 This validator is not registered for epoch {}. Going to sleep.", epoch
                    );

                    return Err(HotStuffError::NotRegisteredForCurrentEpoch { epoch });
                }
                info!(
                    target: LOG_TARGET,
                    "🌟 This validator is registered for epoch {}.", epoch
                );
            },
        }

        Ok(())
    }

    async fn request_initial_catch_up_sync(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
    ) -> Result<(), HotStuffError> {
        for member in epoch_state.local_committee().shuffled() {
            if member.address != self.local_validator_addr {
                self.on_catch_up_sync
                    .request_sync(epoch_state.epoch(), member.address.clone())
                    .await?;
                break;
            }
        }
        Ok(())
    }

    async fn on_failure(&mut self, context: &str, err: &HotStuffError) {
        self.hooks.on_error(err);
        self.publish_event(HotstuffEvent::Failure {
            message: err.to_string(),
        });
        error!(target: LOG_TARGET, "Error ({}): {}", context, err);
        if let Err(e) = self.pacemaker.stop().await {
            error!(target: LOG_TARGET, "Error while stopping pacemaker: {}", e);
        }
        self.on_inbound_message.clear_buffer();
    }

    /// Read and discard messages. This should be used only when consensus is inactive.
    pub async fn discard_messages(&mut self) {
        loop {
            tokio::select! {
                biased;
                _ = self.shutdown.wait() => {
                    break;
                },
                _ = self.on_inbound_message.discard() => {},
                _ = self.rx_new_transactions.recv() => {}
            }
        }
    }

    async fn on_leader_timeout(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        current_height: NodeHeight,
    ) -> Result<(), HotStuffError> {
        self.hooks.on_leader_timeout(current_height);
        info!(target: LOG_TARGET, "⚠️ {} Leader failure: NEXTSYNCVIEW for epoch {} and current height {}", self.local_validator_addr, epoch_state.epoch(), current_height);
        self.on_next_sync_view
            .handle(epoch_state.epoch(), current_height, epoch_state.local_committee())
            .await?;
        self.publish_event(HotstuffEvent::LeaderTimeout { height: current_height });
        Ok(())
    }

    /// Called when it may be time to propose if this node is the leader for the next view
    async fn on_beat(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        local_claim_public_key: &RistrettoPublicKeyBytes,
    ) -> Result<(), HotStuffError> {
        let (highest_justified, last_proposed) = self.state_store.with_read_tx(|tx| {
            let highest_height = get_highest_seen_justified_view(tx, epoch_state.epoch())?;
            let last_proposed = LastProposed::get(tx, epoch_state.epoch()).optional()?;
            Ok::<_, HotStuffError>((highest_height, last_proposed))
        })?;

        // h + 1 because we have not entered the next view yet after creating the new PC.
        // This will happen when we process the block we are about to propose.
        let next_height = highest_justified + NodeHeight(1);
        if last_proposed.is_some_and(|lp| lp.height >= next_height) {
            // We have already proposed at this height, so we don't need to propose again
            debug!(
                target: LOG_TARGET,
                "⤵️ [on_beat] {} Already proposed at height ({})",
                self.local_validator_addr,
                next_height
            );
            return Ok(());
        }

        // Once the highest view justifies this node as leader, we continue i.e we wait for votes to progress the view.
        // Force beat will ensure that if we don't get votes, we will propose with the current QC.
        if !self.leader_strategy.is_leader(
            &self.local_validator_addr,
            epoch_state.local_committee(),
            highest_justified,
        ) {
            debug!(
                target: LOG_TARGET,
                "🔥 [on_beat] {} Not the leader for height ({})",
                self.local_validator_addr,
                highest_justified
            );
            return Ok(());
        }

        info!(
            target: LOG_TARGET,
            "🔥 [on_beat] {} Local node is the leader for {}, num local members: {}, {}",
            self.local_validator_addr,
            highest_justified,
            epoch_state.local_committee().len(),
            epoch_state.local_committee_info().shard_group()
        );

        let propose_now = self.state_store.with_read_tx(|tx| {
            let highest_block = HighestSeenBlock::get(tx, epoch_state.epoch())?;
            // Propose quickly if there are UTXOs to mint or transactions to propose
            let propose_now = ForeignProposalRecord::has_unconfirmed(tx, epoch_state.epoch())? ||
                BurntUtxo::has_unproposed(tx)? ||
                self.transaction_pool
                    .has_ready_or_pending_transaction_updates(tx, highest_block.block_id())?;

            Ok::<_, HotStuffError>(propose_now)
        })?;

        if !propose_now {
            let current_epoch = self.epoch_manager.current_epoch().await?;
            // Propose quickly if we should end the epoch (i.e base layer epoch > pacemaker epoch)
            if current_epoch == epoch_state.epoch() {
                info!(target: LOG_TARGET, "[on_beat] No transactions to propose. Waiting for a timeout.");
                return Ok(());
            }
        }

        self.propose_now(epoch_state, next_height, false, *local_claim_public_key)
            .await?;

        Ok(())
    }

    /// Called when the block time expires (forced_height == None) or when NEWVIEW quorum (TimeoutCertificate) is
    /// reached (forced_height == Some(_))
    async fn on_force_beat(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        current_height: NodeHeight,
        forced_height: Option<NodeHeight>,
        local_claim_public_key: &RistrettoPublicKeyBytes,
    ) -> Result<(), HotStuffError> {
        let height = match forced_height {
            Some(height) => {
                debug!(target: LOG_TARGET, "🔥 [force_beat {}] leader timeout at {height}", self.local_validator_addr);
                height
            },
            None => self
                .state_store
                .with_read_tx(|tx| get_highest_seen_justified_view(tx, epoch_state.epoch()))?,
        };

        let next_height_to_propose = height + NodeHeight(1);
        let last_proposed = self
            .state_store
            .with_read_tx(|tx| LastProposed::get(tx, epoch_state.epoch()))
            .optional()?;
        if last_proposed.is_some_and(|lp| lp.height >= next_height_to_propose) {
            // We have already proposed at this height, so we don't need to propose again
            debug!(
                target: LOG_TARGET,
                "⤵️ [on_force_beat] {} Already proposed at height ({})",
                self.local_validator_addr,
                next_height_to_propose
            );
            return Ok(());
        }

        // `height` is the highest justified view - check if this node is the leader (i.e. should propose to advance the
        // view to h + 1)
        let is_leader =
            self.leader_strategy
                .is_leader(&self.local_validator_addr, epoch_state.local_committee(), height);

        if !is_leader {
            debug!(
                target: LOG_TARGET,
                "🔥 [force_beat] {} Not the leader for {}, local_committee: {} (current height: {})",
                self.local_validator_addr,
                height,
                epoch_state.local_committee().len(),
                current_height
            );
            return Ok(());
        }

        info!(
            target: LOG_TARGET,
            "🔥 [force_beat] {} Local node is leader for {}, local_committee: {}",
            self.local_validator_addr,
            height,
            epoch_state
                .local_committee()
                .len(),
        );

        self.propose_now(
            epoch_state,
            next_height_to_propose,
            forced_height.is_some(),
            *local_claim_public_key,
        )
        .await?;

        Ok(())
    }

    async fn propose_now(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        next_height: NodeHeight,
        is_timeout: bool,
        local_claim_public_key: RistrettoPublicKeyBytes,
    ) -> Result<(), HotStuffError> {
        // We use the highest seen block - specifically to handle the case where a block is proposed and locally
        // accepted, however, for whatever reason, a new certificate could not be created for it. We still use
        // it at the parent for this block, subsequent certificates will justify it.
        let highest_block = self
            .state_store
            .with_read_tx(|tx| HighestSeenBlock::get(tx, epoch_state.epoch()))?;

        // Do we need to fill in gaps with dummy blocks?
        let mut dummy_block = None;
        let mut propose_high_tc = None;
        if next_height > highest_block.height + NodeHeight(1) {
            let (high_qc, high_tc, parent) = self.state_store.with_read_tx(|tx| {
                let high_qc = HighPc::get(tx, epoch_state.epoch())?;
                let high_qc = ProposalCertificate::get(tx, high_qc.epoch(), high_qc.id())?;
                let high_tc = HighTc::get(tx, epoch_state.epoch())?;
                let high_tc = TimeoutCertificate::get(tx, high_tc.epoch(), high_tc.id())?;
                let block = Block::get(tx, highest_block.block_id())?;
                Ok::<_, HotStuffError>((high_qc, high_tc, block))
            })?;

            propose_high_tc = Some(high_tc);

            info!(
                target: LOG_TARGET,
                "⚠️ Leader Failure: Next height is {next_height} but the highest block is {highest_block}. Proposing with dummy blocks to fill the gap.",
            );

            if let Some(dummy) = calculate_last_dummy_block(
                highest_block.height,
                next_height,
                self.config.network,
                epoch_state.epoch(),
                parent.shard_group(),
                *parent.id(),
                &high_qc,
                *parent.state_merkle_root(),
                &self.leader_strategy,
                epoch_state.local_committee(),
                parent.timestamp(),
                *parent.epoch_hash(),
            ) {
                dummy_block = Some(dummy);
            }
        } else if is_timeout {
            // If this is a timeout (without dummies because the highest block is the parent), we need to propose with
            // the highest timeout certificate
            let high_tc = self.state_store.with_read_tx(|tx| {
                let high_tc = HighTc::get(tx, epoch_state.epoch())?;
                TimeoutCertificate::get(tx, high_tc.epoch(), high_tc.id())
            })?;
            propose_high_tc = Some(high_tc);
        } else {
            // Nothing to do
        }

        // TODO: suggest adding self.epoch_manager.did_epoch_change_recently() and only propose when that is not the
        // case - allow some arb time for the network to recognise the new epoch. Currently we often get leader failures
        // due to quick end epoch proposals
        let current_epoch = self.epoch_manager.current_epoch().await?;
        let propose_epoch_end = current_epoch > epoch_state.epoch();
        if propose_epoch_end {
            info!(
                target: LOG_TARGET,
                "🌟 Can propose end of epoch {}->{}",
                epoch_state.epoch(),
                current_epoch
            );
        }

        self.on_propose
            .handle(
                epoch_state,
                next_height,
                local_claim_public_key,
                highest_block,
                dummy_block,
                propose_high_tc,
                propose_epoch_end,
            )
            .await?;

        Ok(())
    }

    async fn dispatch_hotstuff_message(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        current_height: NodeHeight,
        from: TConsensusSpec::Addr,
        msg: HotstuffMessage,
    ) -> Result<(), HotStuffError> {
        match msg {
            HotstuffMessage::NewView(message) => log_err(
                "on_receive_new_view",
                self.on_receive_new_view
                    .handle(epoch_state, current_height, from, *message)
                    .await,
            ),
            HotstuffMessage::Proposal(msg) => log_err(
                "on_receive_local_proposal",
                self.on_proposal_message(epoch_state, current_height, *msg).await,
            ),
            HotstuffMessage::ForeignProposal(msg) => log_err(
                "on_receive_foreign_proposal (received)",
                self.on_receive_foreign_proposal
                    .handle_received(msg, epoch_state.local_committee_info())
                    .await,
            ),
            HotstuffMessage::ForeignProposalNotification(msg) => log_err(
                "on_receive_foreign_proposal (notification)",
                self.on_receive_foreign_proposal
                    .handle_notification_received(from, epoch_state.epoch(), msg, epoch_state.local_committee_info())
                    .await,
            ),
            HotstuffMessage::ForeignProposalRequest(msg) => log_err(
                "on_receive_foreign_proposal (request)",
                self.on_receive_foreign_proposal.handle_requested(from, msg).await,
            ),
            HotstuffMessage::Vote(msg) => log_err(
                "on_receive_vote",
                self.on_receive_vote
                    .handle(from, current_height, epoch_state, msg)
                    .await,
            ),
            HotstuffMessage::MissingTransactionsRequest(msg) => log_err(
                "on_receive_request_missing_transactions",
                self.on_receive_request_missing_txs.handle(from, msg).await,
            ),
            HotstuffMessage::MissingTransactionsResponse(msg) => log_err(
                "on_receive_new_transaction",
                self.on_receive_new_transaction
                    .process_requested(epoch_state.epoch(), from, msg, epoch_state.local_committee_info())
                    .await,
            ),
            HotstuffMessage::CatchUpSyncRequest(msg) => {
                self.on_sync_request.handle(from, epoch_state.epoch(), msg);
                Ok(())
            },
        }
    }

    async fn on_proposal_message(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        current_height: NodeHeight,
        msg: ProposalMessage,
    ) -> Result<(), HotStuffError> {
        let proposed_by = *msg.block.proposed_by();
        match log_err(
            "on_receive_local_proposal",
            self.on_receive_local_proposal.handle(epoch_state, msg).await,
        ) {
            Ok(None) | Ok(Some(NoVoteReason::AlreadyVotedAtHeight)) => Ok(()),
            Ok(Some(_)) => {
                // We decided NOVOTE, so we immediately send a NEWVIEW
                self.on_leader_timeout(epoch_state, current_height).await
            },
            Err(err) => self.handle_hotstuff_error(epoch_state, Some(proposed_by), err).await,
        }
    }

    async fn handle_hotstuff_error(
        &mut self,
        local_epoch_state: &EpochState<TConsensusSpec::Addr>,
        catch_up_from: Option<RistrettoPublicKeyBytes>,
        err: HotStuffError,
    ) -> Result<(), HotStuffError> {
        self.hooks.on_error(&err);
        let (remote_epoch, remote_height) = match &err {
            HotStuffError::FallenBehind {
                qc_epoch, qc_height, ..
            } => (*qc_epoch, *qc_height),
            HotStuffError::ProposalValidationError(ProposalValidationError::JustifyBlockNotFound {
                justify_block,
                ..
            }) => (justify_block.epoch(), justify_block.height()),
            HotStuffError::ProposalValidationError(err) => {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ Proposal validation error: {err}."
                );
                // Failed validations should  not crash consensus
                return Ok(());
            },
            _ => {
                // Other errors can pass though
                return Err(err);
            },
        };

        if remote_epoch > local_epoch_state.epoch() {
            // Valid remote certificate is in a future epoch, so we are behind
            warn!(
                target: LOG_TARGET,
                "❌ Justify block {remote_epoch}/{remote_height} is in a future epoch > current epoch {}. State sync required.",
                local_epoch_state.epoch()
            );
            // Sync
            return Err(err);
        }
        // Otherwise, catch up
        let vn = match catch_up_from {
            Some(pk) => {
                self.epoch_manager
                    .get_validator_node_by_public_key(local_epoch_state.epoch(), pk)
                    .await?
            },
            None => {
                self.epoch_manager
                    .get_random_committee_member(
                        local_epoch_state.epoch(),
                        Some(local_epoch_state.local_committee_info.shard_group()),
                        vec![self.local_validator_addr.clone()],
                    )
                    .await?
            },
        };

        warn!(
            target: LOG_TARGET,
            "⚠️This node has fallen behind due to a missing justified block: {err}. Catching up"
        );
        self.on_catch_up_sync
            .request_sync(local_epoch_state.epoch(), vn.address)
            .await?;
        Ok(())
    }

    fn create_genesis_block_if_required(
        &self,
        epoch: Epoch,
        epoch_hash: FixedHash,
        shard_group: ShardGroup,
    ) -> Result<(), HotStuffError> {
        self.state_store.with_write_tx(|tx| {
            // The parent for genesis blocks refer to this zero block
            let mut zero_block = Block::zero_block(self.config.network, self.config.consensus_constants.num_preshards);
            if !zero_block.exists(&**tx)? {
                debug!(target: LOG_TARGET, "Creating zero block");
                zero_block.justify().save(tx)?;
                zero_block.insert(tx)?;
                zero_block.add_justify_qc(tx, &QcId::zero())?;
                zero_block.commit_block_without_state_changes(tx, &zero_block.justify().calculate_id())?;
            }

            let state_merkle_root = ShardedStateTree::new(&**tx).calculate_state_root(shard_group)?;

            let mut genesis = Block::genesis(
                self.config.network,
                epoch,
                epoch_hash,
                shard_group,
                FixedHash::new(state_merkle_root.into_array()),
                self.config.sidechain_id,
            );
            if !genesis.exists(&**tx)? {
                info!(target: LOG_TARGET, "✨Creating genesis block {genesis}");
                genesis.justify().save(tx)?;
                genesis.insert(tx)?;
                genesis.add_justify_qc(tx, &QcId::zero())?;
                genesis.as_locked().set(tx)?;
                genesis.as_leaf().set(tx)?;
                genesis.as_highest_seen().set(tx)?;
                genesis.as_last_executed().set(tx)?;
                genesis.as_last_voted().set(tx)?;
                genesis.justify().as_high_pc().set(tx)?;
                genesis.commit_block_without_state_changes(tx, &genesis.justify().calculate_id())?;
            }

            Ok(())
        })
    }

    fn publish_event(&self, event: HotstuffEvent) {
        let _ignore = self.tx_events.send(event);
    }
}

impl<TConsensusSpec: ConsensusSpec> Debug for HotstuffWorker<TConsensusSpec> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotstuffWorker")
            .field("validator_addr", &self.local_validator_addr)
            .field("epoch_manager", &"EpochManager")
            .field("pacemaker_handle", &self.pacemaker)
            .field("pacemaker", &"Pacemaker")
            .field("shutdown", &self.shutdown)
            .finish()
    }
}

fn log_err<T>(context: &'static str, result: Result<T, HotStuffError>) -> Result<T, HotStuffError> {
    if let Err(ref e) = result {
        error!(target: LOG_TARGET, "Error while processing new hotstuff message ({context}): {e}");
    }
    result
}
