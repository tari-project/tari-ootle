//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{HashSet, VecDeque};

use log::*;
use tari_bor::BorError;
use tari_consensus_types::{BlockId, LockedBlock};
use tari_epoch_manager::EpochManagerReader;
use tari_ootle_common_types::{
    Epoch,
    NodeHeight,
    committee::{Committee, CommitteeInfo},
    optional::Optional,
};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    consensus_models::{
        Block,
        BookkeepingModel,
        ForeignParkedProposal,
        ForeignProposal,
        ProposalOrigin,
        TransactionRecord,
    },
};
use tari_ootle_transaction::{Transaction, TransactionId};
use tokio::sync::broadcast;

use super::config::HotstuffConfig;
use crate::{
    hotstuff::{
        HotstuffEvent,
        ProposalValidationError,
        epoch_state::EpochState,
        error::HotStuffError,
        on_receive_foreign_proposal::{OnReceiveForeignProposalHandler, validate_foreign_proposal_contents},
        on_receive_new_transaction::OnReceiveNewTransaction,
    },
    messages::{
        EncodedTransaction,
        ForeignProposalMessage,
        ForeignProposalRequestMessage,
        HotstuffMessage,
        MAX_REQUESTED_TRANSACTIONS,
        MissingTransactionsRequest,
        MissingTransactionsResponse,
        ProposalMessage,
    },
    tracing::TraceTimer,
    traits::{ConsensusSpec, OutboundMessaging},
    validations,
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_message_validate";

/// How many blocks waiting on transactions one proposer may have parked at one height. An honest leader proposes once
/// per height, so the allowance leaves room for a re-proposal. Slots are counted per proposer so that one member's
/// blocks cannot take another member's.
pub const MAX_PARKED_BLOCKS_PER_PROPOSER_PER_HEIGHT: usize = 2;

pub struct OnMessageValidate<TConsensusSpec: ConsensusSpec> {
    config: HotstuffConfig,
    store: TConsensusSpec::StateStore,
    epoch_manager: TConsensusSpec::EpochManager,
    vote_signing_service: TConsensusSpec::SignerService,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
    tx_events: broadcast::WeakSender<HotstuffEvent>,
    /// Keep track of max 32 in-flight requests
    active_missing_transaction_requests: MissingTransactionRequests<TConsensusSpec::Addr>,
    current_request_id: u32,
}

impl<TConsensusSpec: ConsensusSpec> OnMessageValidate<TConsensusSpec> {
    pub fn new(
        config: HotstuffConfig,
        store: TConsensusSpec::StateStore,
        epoch_manager: TConsensusSpec::EpochManager,
        vote_signing_service: TConsensusSpec::SignerService,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
        tx_events: broadcast::WeakSender<HotstuffEvent>,
    ) -> Self {
        Self {
            config,
            store,
            epoch_manager,
            vote_signing_service,
            outbound_messaging,
            tx_events,
            active_missing_transaction_requests: MissingTransactionRequests::new(),
            current_request_id: 0,
        }
    }

    pub async fn handle(
        &mut self,
        current_epoch: Epoch,
        current_height: NodeHeight,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        from: TConsensusSpec::Addr,
        msg: HotstuffMessage,
        new_transactions: &OnReceiveNewTransaction<TConsensusSpec>,
        foreign_proposals: &mut OnReceiveForeignProposalHandler<TConsensusSpec>,
    ) -> Result<MessageValidationResult<TConsensusSpec::Addr>, HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "on_message_validate");
        match msg {
            HotstuffMessage::Proposal(msg) => {
                if !epoch_state.local_committee().contains(&from) {
                    warn!(
                        target: LOG_TARGET,
                        "❌ Received Proposal from non-committee member {}. Discarding message.",
                        from
                    );
                    return Ok(MessageValidationResult::Discard);
                }
                self.process_local_proposal(current_epoch, current_height, from, epoch_state, *msg, new_transactions)
                    .await
            },
            HotstuffMessage::CatchUpSyncResponse(msg) => {
                if !epoch_state.local_committee().contains(&from) {
                    warn!(
                        target: LOG_TARGET,
                        "❌ Received CatchUpSyncResponse from non-committee member {}. Discarding message.",
                        from
                    );
                    return Ok(MessageValidationResult::Discard);
                }
                self.process_catch_up_response(current_epoch, from, epoch_state, *msg, new_transactions)
                    .await
            },
            HotstuffMessage::ForeignProposal(msg) => {
                // Honest late replies to a request another peer already answered also land here, so this logs at debug.
                let block_id = msg.proposal.calculate_block_id();
                if !foreign_proposals.take_reply(&block_id, &from) {
                    debug!(target: LOG_TARGET, "❓Received foreign proposal {block_id} from {from}, who has no open request for it. Discarding message");
                    return Ok(MessageValidationResult::Discard);
                }
                self.process_foreign_proposal(epoch_state, from, msg).await
            },
            HotstuffMessage::MissingTransactionsResponse(msg) => {
                Ok(self.process_missing_transactions_response(from, msg))
            },
            HotstuffMessage::MissingTransactionsRequest(msg) => {
                if !self
                    .is_missing_transactions_requester(epoch_state, &from, msg.epoch)
                    .await?
                {
                    return Ok(MessageValidationResult::Discard);
                }
                if msg.transactions.len() > MAX_REQUESTED_TRANSACTIONS {
                    warn!(target: LOG_TARGET, "⚠️Peer requested more than the maximum amount of transactions. Discarding message");
                    return Ok(MessageValidationResult::Discard);
                }
                Ok(MessageValidationResult::Ready {
                    from,
                    message: HotstuffMessage::MissingTransactionsRequest(msg),
                })
            },
            HotstuffMessage::ForeignProposalRequest(msg) => {
                if !self.is_foreign_proposal_requester(epoch_state, &from, &msg).await? {
                    return Ok(MessageValidationResult::Discard);
                }
                Ok(MessageValidationResult::Ready {
                    from,
                    message: HotstuffMessage::ForeignProposalRequest(msg),
                })
            },
            msg @ HotstuffMessage::NewView(_) |
            msg @ HotstuffMessage::Vote(_) |
            msg @ HotstuffMessage::CatchUpSyncRequest(_) => {
                if !epoch_state.local_committee().contains(&from) {
                    warn!(
                        target: LOG_TARGET,
                        "❌ Received {} from non-committee member {}. Discarding message.",
                        msg.as_type_str(),
                        from
                    );
                    return Ok(MessageValidationResult::Discard);
                }
                Ok(MessageValidationResult::Ready { from, message: msg })
            },
            msg => Ok(MessageValidationResult::Ready { from, message: msg }),
        }
    }

    fn process_missing_transactions_response(
        &mut self,
        from: TConsensusSpec::Addr,
        msg: MissingTransactionsResponse,
    ) -> MessageValidationResult<TConsensusSpec::Addr> {
        let Some(request) = self.active_missing_transaction_requests.take(msg.request_id, &from) else {
            warn!(target: LOG_TARGET, "❓Received missing transactions (req_id = {}) from {} that we did not request. Discarding message", msg.request_id, from);
            return MessageValidationResult::Discard;
        };

        match request.decode_response(
            msg.transactions,
            self.config.consensus_constants.max_transaction_size_bytes,
        ) {
            Ok(transactions) => {
                let remaining = request.remaining_after(&transactions);
                MessageValidationResult::RequestedTransactions {
                    from,
                    block_id: request.block_id,
                    epoch: request.epoch,
                    transactions,
                    remaining,
                }
            },
            Err(err) => {
                warn!(target: LOG_TARGET, "⚠️Peer {from} sent an invalid response to req_id = {}: {err}. Discarding message", msg.request_id);
                MessageValidationResult::Discard
            },
        }
    }

    /// Local committee members request transactions for local proposals and foreign committee members for foreign
    /// proposals, so any validator registered for the request's epoch may ask.
    async fn is_missing_transactions_requester(
        &self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        from: &TConsensusSpec::Addr,
        epoch: Epoch,
    ) -> Result<bool, HotStuffError> {
        if epoch_state.local_committee().contains(from) {
            return Ok(true);
        }

        // The epoch is chosen by the requester, so a failed lookup rejects the request rather than the worker.
        match self
            .epoch_manager
            .get_committee_info_by_validator_address(epoch, from)
            .await
        {
            Ok(_) => Ok(true),
            Err(err) => {
                warn!(
                    target: LOG_TARGET,
                    "❌ Received MissingTransactionsRequest from {from} who is not a registered validator for epoch \
                     {epoch} ({err}). Discarding message.",
                );
                Ok(false)
            },
        }
    }

    /// A foreign proposal is only served to a registered validator that is a member of the foreign shard group it asks
    /// pledges for.
    async fn is_foreign_proposal_requester(
        &self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        from: &TConsensusSpec::Addr,
        msg: &ForeignProposalRequestMessage,
    ) -> Result<bool, HotStuffError> {
        let ForeignProposalRequestMessage::ByBlockId {
            for_shard_group, epoch, ..
        } = msg;

        if *for_shard_group == epoch_state.local_committee_info().shard_group() {
            warn!(
                target: LOG_TARGET,
                "❌ Received ForeignProposalRequest from {from} for the local shard group {for_shard_group}. Discarding message.",
            );
            return Ok(false);
        }

        // `for_shard_group` is the requester's shard group in its own current epoch, which may be one either side of
        // ours across a boundary, and shard groups move between epochs when the committee count changes. The epoch is
        // chosen by the requester, so a failed lookup counts as no match.
        let current_epoch = epoch_state.epoch();
        let mut candidate_epochs = vec![current_epoch];
        for candidate in [
            Some(*epoch),
            Some(current_epoch + Epoch(1)),
            current_epoch.checked_sub(Epoch(1)),
        ]
        .into_iter()
        .flatten()
        {
            if !candidate_epochs.contains(&candidate) {
                candidate_epochs.push(candidate);
            }
        }
        for candidate in candidate_epochs {
            if let Ok(requester) = self
                .epoch_manager
                .get_committee_info_by_validator_address(candidate, from)
                .await &&
                requester.shard_group() == *for_shard_group
            {
                return Ok(true);
            }
        }

        warn!(
            target: LOG_TARGET,
            "❌ Received ForeignProposalRequest from {from} who is not a member of shard group {for_shard_group} in or \
             around epoch {current_epoch} (requested epoch {epoch}). Discarding message.",
        );
        Ok(false)
    }

    pub async fn request_missing_transactions(
        &mut self,
        to: TConsensusSpec::Addr,
        block_id: BlockId,
        epoch: Epoch,
        missing_txs: HashSet<TransactionId>,
    ) -> Result<(), HotStuffError> {
        let request_id = self.next_request_id();
        self.outbound_messaging
            .send(
                to.clone(),
                HotstuffMessage::MissingTransactionsRequest(MissingTransactionsRequest {
                    request_id,
                    block_id,
                    epoch,
                    transactions: missing_txs.clone(),
                }),
            )
            .await?;
        // Only a request that went out holds one of the few slots a response can be matched against.
        self.active_missing_transaction_requests
            .insert(PendingMissingTransactionsRequest {
                request_id,
                to,
                block_id,
                epoch,
                transactions: missing_txs,
            });
        Ok(())
    }

    fn next_request_id(&mut self) -> u32 {
        let req_id = self.current_request_id;
        self.current_request_id = self.current_request_id.wrapping_add(1);
        req_id
    }

    async fn process_local_proposal(
        &mut self,
        current_epoch: Epoch,
        current_height: NodeHeight,
        from: TConsensusSpec::Addr,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        proposal: ProposalMessage,
        new_transactions: &OnReceiveNewTransaction<TConsensusSpec>,
    ) -> Result<MessageValidationResult<TConsensusSpec::Addr>, HotStuffError> {
        info!(
            target: LOG_TARGET,
            "📜 new unvalidated PROPOSAL message {} from {} (current height = {})",
            proposal.block,
            proposal.block.proposed_by(),
            current_height,
        );

        if proposal.block.height() < current_height {
            // Should never happen since the on_inbound_message handler filters these out
            info!(
                target: LOG_TARGET,
                "🔥 Block {} is lower than current height {}. Ignoring.",
                proposal.block,
                current_height
            );
            return Ok(MessageValidationResult::Discard);
        }

        if let Err(err) = self.check_local_proposal(current_epoch, &proposal.block, epoch_state) {
            return Ok(MessageValidationResult::Invalid {
                from,
                message: HotstuffMessage::new_proposal(proposal),
                err,
            });
        }

        self.handle_missing_transactions_local_block(
            from,
            epoch_state,
            proposal,
            new_transactions,
            ProposalOrigin::Live,
        )
        .await
    }

    /// Validate a block delivered via catch-up sync. This is an ordered block import, so — unlike
    /// [`Self::process_local_proposal`] — it is intentionally NOT gated on the current view height:
    /// a node whose view has advanced beyond its stored blocks must still ingest the gap. Stateless
    /// validation and missing-transaction parking are identical to the live-proposal path.
    async fn process_catch_up_response(
        &mut self,
        current_epoch: Epoch,
        from: TConsensusSpec::Addr,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        proposal: ProposalMessage,
        new_transactions: &OnReceiveNewTransaction<TConsensusSpec>,
    ) -> Result<MessageValidationResult<TConsensusSpec::Addr>, HotStuffError> {
        info!(
            target: LOG_TARGET,
            "🌐 new unvalidated CATCH-UP block {} from {}",
            proposal.block,
            from,
        );

        if let Err(err) = self.check_local_proposal(current_epoch, &proposal.block, epoch_state) {
            return Ok(MessageValidationResult::Invalid {
                from,
                message: HotstuffMessage::new_catch_up_sync_response(proposal),
                err,
            });
        }

        self.handle_missing_transactions_local_block(
            from,
            epoch_state,
            proposal,
            new_transactions,
            ProposalOrigin::CatchUp,
        )
        .await
    }

    pub fn update_parked_blocks<'a, I: IntoIterator<Item = &'a TransactionId> + ExactSizeIterator>(
        &self,
        transaction_ids: I,
    ) -> Result<(UnparkedLocalBlocks, Vec<ForeignProposalMessage>), HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "update_local_parked_blocks").with_iterations(transaction_ids.len());
        self.store.with_write_tx(|tx| {
            // TODO(perf)
            let mut unparked_blocks = Vec::new();
            let mut foreign_unparked_blocks = Vec::new();
            for transaction_id in transaction_ids {
                debug!(target: LOG_TARGET, "🔍 Checking if transaction {} unparks any blocks", transaction_id);
                let unparked = tx.parked_block_remove_missing_transaction(transaction_id)?;
                if unparked.is_empty() {
                    debug!(target: LOG_TARGET, "🔍 Transaction {} did not unpark any blocks", transaction_id);
                } else {
                    unparked_blocks.reserve(unparked.len());
                    for (unparked_block, foreign_proposals, origin) in unparked {
                        info!(
                            target: LOG_TARGET,
                            "♻️ all transactions for local block {unparked_block} are ready for consensus"
                        );

                        self.publish_event(HotstuffEvent::ParkedBlockReady {
                            block: unparked_block.as_leaf(),
                        });

                        unparked_blocks.push((
                            ProposalMessage {
                                block: unparked_block,
                                foreign_proposals,
                            },
                            origin,
                        ));
                    }
                }

                let foreign_unparked = ForeignParkedProposal::remove_by_transaction_id(tx, transaction_id)?;
                if foreign_unparked.is_empty() {
                    debug!(target: LOG_TARGET, "🔍 Transaction {} did not unpark any foreign blocks", transaction_id);
                }else{
                    info!(target: LOG_TARGET, "♻️ all transactions for {} foreign block(s) are ready for consensus", foreign_unparked.len());
                    foreign_unparked_blocks.extend(foreign_unparked.into_iter().map(Into::into));
                }
            }
            sort_unparked_local_blocks(&mut unparked_blocks);
            Ok((unparked_blocks, foreign_unparked_blocks))
        })
    }

    fn check_local_proposal(
        &self,
        current_epoch: Epoch,
        block: &Block,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
    ) -> Result<(), HotStuffError> {
        validations::check_local_proposal::<TConsensusSpec>(
            current_epoch,
            block,
            epoch_state.local_committee(),
            epoch_state.local_committee_info(),
            &self.vote_signing_service,
            &self.config,
            epoch_state.epoch_hash(),
            epoch_state.exhaust_burn_rate(),
        )
    }

    fn check_foreign_proposal(
        &self,
        proposal: &ForeignProposal,
        committee: &Committee<TConsensusSpec::Addr>,
    ) -> Result<(), HotStuffError> {
        validations::check_foreign_proposal::<TConsensusSpec>(proposal, committee, &self.config)
    }

    /// Parks the block if it is waiting on transactions this node does not hold, otherwise passes it on as the
    /// message `to_message` builds.
    async fn handle_missing_transactions_local_block(
        &mut self,
        from: TConsensusSpec::Addr,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        proposal: ProposalMessage,
        new_transactions: &OnReceiveNewTransaction<TConsensusSpec>,
        origin: ProposalOrigin,
    ) -> Result<MessageValidationResult<TConsensusSpec::Addr>, HotStuffError> {
        let Some(has_missing_transactions) = self.store.with_write_tx(|tx| {
            if !validations::are_block_foreign_proposals_matched(&**tx, &proposal.block, &proposal.foreign_proposals)? {
                return Ok::<_, HotStuffError>(None);
            }
            // `local_committee_info` comes from `epoch_state` (the node's current epoch), matching
            // `get_missing_transactions` below; `check_local_proposal` has already verified that the block's shard
            // group matches `epoch_state.local_committee_info().shard_group()`, while the transaction's validity
            // window is checked against `proposal.block.epoch()`.
            self.resequence_unpooled_transactions(tx, epoch_state.local_committee_info(), &proposal, new_transactions)?;
            let missing = self.get_missing_transactions(&**tx, epoch_state.local_committee_info(), &proposal)?;
            Ok(Some(!missing.is_empty()))
        })?
        else {
            warn!(
                target: LOG_TARGET,
                "⚠️ Discarding proposal for block {} from {from}: embedded foreign proposals do not match the \
                 block's foreign proposal commands",
                proposal.block,
            );
            return Ok(MessageValidationResult::Discard);
        };

        if !has_missing_transactions {
            return Ok(MessageValidationResult::Ready {
                from,
                message: proposal_message(origin, proposal),
            });
        }

        if !self.has_parking_slot(&from, epoch_state, &proposal.block)? {
            return Ok(MessageValidationResult::Discard);
        }

        if !self
            .are_embedded_foreign_proposals_parkable(epoch_state, &proposal)
            .await?
        {
            return Ok(MessageValidationResult::Discard);
        }

        let missing_tx_ids = self.store.with_write_tx(|tx| {
            self.check_for_missing_transactions(tx, epoch_state.local_committee_info(), &proposal, origin)
        })?;

        if missing_tx_ids.is_empty() {
            return Ok(MessageValidationResult::Ready {
                from,
                message: proposal_message(origin, proposal),
            });
        }

        self.publish_event(HotstuffEvent::ProposedBlockParked {
            block: proposal.block.as_leaf(),
            num_missing_txs: missing_tx_ids.len(),
            // TODO: remove
            num_awaiting_txs: 0,
        });

        Ok(MessageValidationResult::ParkedProposal {
            block_id: *proposal.block.id(),
            epoch: proposal.block.epoch(),
            missing_txs: missing_tx_ids,
        })
    }

    /// Rebuilds pool records for this block's transactions that we hold but that are absent from the
    /// transaction pool, so that a proposal is not rejected over derived state no peer can resend.
    ///
    /// Only the commands the rebuilt `New` stage can serve are offered: a later command is not made
    /// votable by a `New` record, and that record would then be proposed at a stage the committee has
    /// moved past. See [`OnReceiveNewTransaction::resequence_known_transactions`].
    fn resequence_unpooled_transactions(
        &self,
        tx: &mut <TConsensusSpec::StateStore as StateStore>::WriteTransaction<'_>,
        local_committee_info: &CommitteeInfo,
        proposal: &ProposalMessage,
        new_transactions: &OnReceiveNewTransaction<TConsensusSpec>,
    ) -> Result<(), HotStuffError> {
        let ids = proposal
            .block
            .commands()
            .iter()
            .filter_map(|cmd| {
                cmd.local_only()
                    .map(|atom| atom.id)
                    .or_else(|| cmd.local_prepare().map(|atom| atom.id))
            })
            .collect::<Vec<_>>();

        if ids.is_empty() {
            return Ok(());
        }

        new_transactions.resequence_known_transactions(tx, proposal.block.epoch(), ids, local_committee_info)?;
        Ok(())
    }

    fn get_missing_transactions<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        local_committee_info: &CommitteeInfo,
        proposal: &ProposalMessage,
    ) -> Result<HashSet<TransactionId>, HotStuffError> {
        if proposal.block.commands().is_empty() {
            debug!(
                target: LOG_TARGET,
                "✅ Block {} is empty (no missing transactions)", proposal.block
            );
            return Ok(HashSet::new());
        }
        let mut missing_tx_ids = TransactionRecord::get_missing(tx, proposal.block.all_transaction_ids())?;
        // Also park block if it has missing transactions from foreign proposals
        for proposal in &proposal.foreign_proposals {
            let foreign_missing =
                self.get_missing_transactions_for_foreign_proposal(tx, local_committee_info, proposal)?;
            missing_tx_ids.extend(foreign_missing);
        }
        Ok(missing_tx_ids)
    }

    /// Parked blocks are kept until their transactions arrive or the chain locks past them, so a block is only parked
    /// if a local committee member proposed it above the locked block, and that member has a slot left at its height.
    /// A block already parked keeps its slot.
    fn has_parking_slot(
        &self,
        from: &TConsensusSpec::Addr,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        block: &Block,
    ) -> Result<bool, HotStuffError> {
        if !epoch_state.local_committee().contains_public_key(block.proposed_by()) {
            warn!(
                target: LOG_TARGET,
                "⚠️ Not parking block {block} from {from}: proposer {} is not a local committee member",
                block.proposed_by(),
            );
            return Ok(false);
        }

        self.store.with_read_tx(|tx| {
            if tx.parked_block_exists(block.id())? {
                return Ok(true);
            }

            if let Some(locked) = LockedBlock::get(tx, block.epoch()).optional()? &&
                block.height() <= locked.height()
            {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ Not parking block {block} from {from}: it is at or below the locked block {locked}",
                );
                return Ok(false);
            }

            let num_parked = tx.parked_blocks_count_by_proposer(block.epoch(), block.height(), block.proposed_by())?;
            if num_parked >= MAX_PARKED_BLOCKS_PER_PROPOSER_PER_HEIGHT {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ Not parking block {block} from {from}: proposer {} already has {num_parked} parked block(s) at \
                     this height",
                    block.proposed_by(),
                );
                return Ok(false);
            }

            Ok(true)
        })
    }

    /// The first parked message carrying a block holds its slot until it is unparked, so a block is only parked if
    /// every embedded foreign proposal whose commit proof is not already stored is authenticated by its committee,
    /// pledges the values its evidence commits to and carries in full every command the local shard group needs. A
    /// message that fails is turned away, leaving the slot for another delivery of the block.
    async fn are_embedded_foreign_proposals_parkable(
        &self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        proposal: &ProposalMessage,
    ) -> Result<bool, HotStuffError> {
        for foreign_proposal in &proposal.foreign_proposals {
            if self
                .store
                .with_read_tx(|tx| validations::is_authenticated_commit_proof_stored(tx, foreign_proposal))?
            {
                continue;
            }

            let rejection = match validations::authenticate_foreign_proposal::<TConsensusSpec>(
                &self.epoch_manager,
                foreign_proposal,
                &self.config,
            )
            .await
            {
                Ok(()) => validate_foreign_proposal_contents(
                    foreign_proposal,
                    epoch_state.local_committee_info().shard_group(),
                )
                .err()
                .filter(|err| err.rejects_message_not_block()),
                Err(HotStuffError::ProposalValidationError(err)) => Some(err),
                Err(err) => return Err(err),
            };
            if let Some(err) = rejection {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ Not parking block {}: embedded foreign proposal {} failed validation: {}",
                    proposal.block,
                    foreign_proposal,
                    err,
                );
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn check_for_missing_transactions(
        &self,
        tx: &mut <TConsensusSpec::StateStore as StateStore>::WriteTransaction<'_>,
        local_committee_info: &CommitteeInfo,
        proposal: &ProposalMessage,
        origin: ProposalOrigin,
    ) -> Result<HashSet<TransactionId>, HotStuffError> {
        let missing_tx_ids = self.get_missing_transactions(&**tx, local_committee_info, proposal)?;

        if missing_tx_ids.is_empty() {
            debug!(
                target: LOG_TARGET,
                "✅ Block {} has no missing transactions", proposal.block
            );
            return Ok(HashSet::new());
        }

        info!(
            target: LOG_TARGET,
            "⏳ Block {} has {} missing transactions", proposal.block, missing_tx_ids.len(),
        );

        tx.parked_block_insert(&proposal.block, &proposal.foreign_proposals, origin, &missing_tx_ids)?;

        Ok(missing_tx_ids)
    }

    async fn process_foreign_proposal(
        &self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        from: TConsensusSpec::Addr,
        msg: ForeignProposalMessage,
    ) -> Result<MessageValidationResult<TConsensusSpec::Addr>, HotStuffError> {
        info!(
            target: LOG_TARGET,
            "🧩 new unvalidated FOREIGN PROPOSAL message {} from {}",
            msg,
            from
        );

        if msg.proposal.commit_proof().commands().is_empty() {
            warn!(
                target: LOG_TARGET,
                "❌ Foreign proposal block {} is empty; therefore, it cannot involve the local shard group", msg.proposal,
            );
            let block_id = msg.proposal.calculate_block_id();
            return Ok(MessageValidationResult::Invalid {
                from,
                message: HotstuffMessage::ForeignProposal(msg),
                err: ProposalValidationError::NoTransactionsInCommittee { block_id }.into(),
            });
        }

        if msg.proposal.shard_group_checked() == Some(epoch_state.local_committee_info().shard_group()) {
            warn!(
                target: LOG_TARGET,
                "⚠️ Foreign proposal block {} from {from} names the local shard group {}; discarding message",
                msg.proposal,
                epoch_state.local_committee_info().shard_group(),
            );
            return Ok(MessageValidationResult::Discard);
        }

        let committee = match validations::resolve_foreign_committee(&self.epoch_manager, &msg.proposal).await {
            Ok(Some(committee)) => committee,
            Ok(None) => return Ok(MessageValidationResult::Discard),
            Err(HotStuffError::ProposalValidationError(err)) => {
                return Ok(MessageValidationResult::Invalid {
                    from,
                    message: HotstuffMessage::ForeignProposal(msg),
                    err: err.into(),
                });
            },
            Err(err) => return Err(err),
        };

        if let Err(err) = self.check_foreign_proposal(&msg.proposal, &committee) {
            return Ok(MessageValidationResult::Invalid {
                from,
                message: HotstuffMessage::ForeignProposal(msg),
                err,
            });
        }

        self.admit_or_park_foreign_proposal(epoch_state.local_committee_info(), from, msg)
    }

    fn admit_or_park_foreign_proposal(
        &self,
        local_committee_info: &CommitteeInfo,
        from: TConsensusSpec::Addr,
        msg: ForeignProposalMessage,
    ) -> Result<MessageValidationResult<TConsensusSpec::Addr>, HotStuffError> {
        self.store.with_write_tx(|tx| {
            let all_involved_transactions = msg
                .proposal
                .all_transaction_ids_in_committee(local_committee_info);
            // CASE: all foreign proposals must include evidence
            let num_transactions = all_involved_transactions.clone().count();
            if num_transactions == 0 {
                warn!(
                    target: LOG_TARGET,
                    "❌ Foreign Block {} has no transactions involving our committee", msg.proposal
                );
                // drop the borrow of msg.block
                drop(all_involved_transactions);
                let block_id = msg.proposal.calculate_block_id();
                return Ok(MessageValidationResult::Invalid {
                    from,
                    message: HotstuffMessage::ForeignProposal(msg),
                    err: ProposalValidationError::NoTransactionsInCommittee { block_id }.into(),
                });
            }

            let missing_tx_ids = TransactionRecord::get_missing(&**tx, all_involved_transactions)?;

            if missing_tx_ids.is_empty() {
                debug!(
                    target: LOG_TARGET,
                    "✅ Foreign Block {} has no missing transactions (out of {} transaction(s) involving this shard group)", msg.proposal,
                    num_transactions
                );
                return Ok(MessageValidationResult::Ready {
                    from,
                    message: HotstuffMessage::ForeignProposal(msg),
                });
            }

            info!(
                target: LOG_TARGET,
                "⏳ Foreign Block {} has {} missing transactions", msg.proposal, missing_tx_ids.len(),
            );

            // The first parked message carrying a block holds its slot until unparked, so a message that withholds a
            // command or whose pledges its evidence does not commit to is turned away here, leaving the slot for
            // another delivery of the block.
            if let Err(err) = validate_foreign_proposal_contents(&msg.proposal, local_committee_info.shard_group()) &&
                err.rejects_message_not_block()
            {
                warn!(target: LOG_TARGET, "⚠️ Discarding foreign proposal from {from}: {err}");
                return Ok(MessageValidationResult::Discard);
            }

            let parked_block = ForeignParkedProposal::from(msg);
            if parked_block.save(tx)? {
                parked_block.add_missing_transactions(tx, &missing_tx_ids)?;
            }

            Ok(MessageValidationResult::ParkedProposal {
                block_id: *parked_block.block_id(),
                epoch: parked_block.epoch(),
                missing_txs: missing_tx_ids,
            })
        })
    }

    fn get_missing_transactions_for_foreign_proposal<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        local_committee_info: &CommitteeInfo,
        proposal: &ForeignProposal,
    ) -> Result<HashSet<TransactionId>, HotStuffError> {
        let mut all_involved_transactions = proposal
            .all_transaction_ids_in_committee(local_committee_info)
            .peekable();

        if all_involved_transactions.peek().is_none() {
            return Ok(HashSet::new());
        }

        let missing = TransactionRecord::get_missing(tx, all_involved_transactions)?;

        Ok(missing)
    }

    fn publish_event(&self, event: HotstuffEvent) {
        if let Some(sender) = self.tx_events.upgrade() {
            let _ignore = sender.send(event);
        }
    }
}

#[derive(Debug)]
pub enum MessageValidationResult<TAddr> {
    Ready {
        from: TAddr,
        message: HotstuffMessage,
    },
    /// The decoded transactions of a response to a missing-transactions request this node made.
    RequestedTransactions {
        from: TAddr,
        block_id: BlockId,
        epoch: Epoch,
        transactions: Vec<Transaction>,
        /// The requested transactions the response did not carry.
        remaining: HashSet<TransactionId>,
    },
    ParkedProposal {
        block_id: BlockId,
        epoch: Epoch,
        missing_txs: HashSet<TransactionId>,
    },
    Discard,
    Invalid {
        from: TAddr,
        message: HotstuffMessage,
        err: HotStuffError,
    },
}

/// A missing-transaction request we sent and are still willing to accept a response for.
#[derive(Debug, Clone)]
struct PendingMissingTransactionsRequest<TAddr> {
    request_id: u32,
    /// The peer the request went to. A response only counts when it comes back from this peer.
    to: TAddr,
    block_id: BlockId,
    epoch: Epoch,
    transactions: HashSet<TransactionId>,
}

impl<TAddr> PendingMissingTransactionsRequest<TAddr> {
    /// Decodes the transactions a peer returned for this request, one at a time, refusing the response at the first
    /// transaction that is over `max_transaction_size_bytes`, was not requested or is returned twice. Only
    /// transactions this request names are ever held decoded, so the heap a response costs is bounded by the
    /// transactions this node asked for.
    fn decode_response(
        &self,
        transactions: Vec<EncodedTransaction>,
        max_transaction_size_bytes: usize,
    ) -> Result<Vec<Transaction>, RequestedTransactionsError> {
        if transactions.len() > self.transactions.len() {
            return Err(RequestedTransactionsError::TooMany {
                returned: transactions.len(),
                requested: self.transactions.len(),
            });
        }

        let mut returned_ids = HashSet::with_capacity(transactions.len());
        let mut decoded = Vec::with_capacity(transactions.len());
        for encoded in transactions {
            if encoded.len() > max_transaction_size_bytes {
                return Err(RequestedTransactionsError::TooLarge {
                    size: encoded.len(),
                    max: max_transaction_size_bytes,
                });
            }
            let transaction = encoded.decode().map_err(RequestedTransactionsError::Malformed)?;
            let id = transaction.calculate_id();
            if !self.transactions.contains(&id) {
                return Err(RequestedTransactionsError::Unrequested { transaction_id: id });
            }
            if !returned_ids.insert(id) {
                return Err(RequestedTransactionsError::Duplicate { transaction_id: id });
            }
            decoded.push(transaction);
        }
        Ok(decoded)
    }

    /// The requested transactions that `returned` does not include.
    fn remaining_after(&self, returned: &[Transaction]) -> HashSet<TransactionId> {
        let mut remaining = self.transactions.clone();
        for transaction in returned {
            remaining.remove(&transaction.calculate_id());
        }
        remaining
    }
}

#[derive(Debug, thiserror::Error)]
enum RequestedTransactionsError {
    #[error("returned {returned} transaction(s) but only {requested} were requested")]
    TooMany { returned: usize, requested: usize },
    #[error("returned a {size} byte transaction, over the {max} byte limit")]
    TooLarge { size: usize, max: usize },
    #[error("returned a transaction that does not decode: {0}")]
    Malformed(BorError),
    #[error("returned transaction {transaction_id}, which was not requested")]
    Unrequested { transaction_id: TransactionId },
    #[error("returned transaction {transaction_id} more than once")]
    Duplicate { transaction_id: TransactionId },
}

/// A fixed-capacity ring of in-flight missing-transaction requests. Capacity is a bound on concurrent
/// requests, not a guarantee: the oldest entry is dropped once it is full, and its response is then
/// discarded as unrequested.
#[derive(Debug, Clone)]
struct MissingTransactionRequests<TAddr> {
    requests: VecDeque<PendingMissingTransactionsRequest<TAddr>>,
}

impl<TAddr: PartialEq> MissingTransactionRequests<TAddr> {
    const CAPACITY: usize = 32;

    pub fn new() -> Self {
        Self {
            requests: VecDeque::with_capacity(Self::CAPACITY),
        }
    }

    pub fn insert(&mut self, request: PendingMissingTransactionsRequest<TAddr>) {
        if self.requests.len() == Self::CAPACITY {
            self.requests.pop_front();
        }
        self.requests.push_back(request);
    }

    /// Removes and returns the request with this id that was sent to `from`, if any.
    pub fn take(&mut self, request_id: u32, from: &TAddr) -> Option<PendingMissingTransactionsRequest<TAddr>> {
        let pos = self
            .requests
            .iter()
            .position(|req| req.request_id == request_id && req.to == *from)?;
        self.requests.remove(pos)
    }
}

type UnparkedLocalBlocks = Vec<(ProposalMessage, ProposalOrigin)>;

fn sort_unparked_local_blocks(blocks: &mut [(ProposalMessage, ProposalOrigin)]) {
    blocks.sort_by_key(|(msg, _)| (msg.block.epoch(), msg.block.height()));
}

fn proposal_message(origin: ProposalOrigin, proposal: ProposalMessage) -> HotstuffMessage {
    match origin {
        ProposalOrigin::Live => HotstuffMessage::new_proposal(proposal),
        ProposalOrigin::CatchUp => HotstuffMessage::new_catch_up_sync_response(proposal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(n: u8) -> HashSet<TransactionId> {
        [TransactionId::new([n; 32])].into_iter().collect()
    }

    fn pending(
        request_id: u32,
        to: &'static str,
        transactions: HashSet<TransactionId>,
    ) -> PendingMissingTransactionsRequest<&'static str> {
        PendingMissingTransactionsRequest {
            request_id,
            to,
            block_id: BlockId::zero(),
            epoch: Epoch(1),
            transactions,
        }
    }

    #[test]
    fn a_response_from_a_peer_we_did_not_ask_is_not_matched() {
        let mut requests = MissingTransactionRequests::new();
        requests.insert(pending(1, "alice", req(1)));

        assert!(requests.take(1, &"bob").is_none());
        assert!(requests.take(1, &"alice").is_some());
    }

    #[test]
    fn the_same_id_to_two_peers_matches_each_peer_once() {
        let mut requests = MissingTransactionRequests::new();
        requests.insert(pending(1, "alice", req(1)));
        requests.insert(pending(1, "bob", req(2)));

        assert_eq!(requests.take(1, &"bob").unwrap().transactions, req(2));
        assert_eq!(requests.take(1, &"alice").unwrap().transactions, req(1));
        assert!(requests.take(1, &"alice").is_none());
    }

    #[test]
    fn the_oldest_request_is_evicted_once_capacity_is_reached() {
        let mut requests = MissingTransactionRequests::new();
        for i in 0..=MissingTransactionRequests::<&str>::CAPACITY {
            requests.insert(pending(i as u32, "alice", req(0)));
        }

        assert!(requests.take(0, &"alice").is_none());
        assert!(
            requests
                .take(MissingTransactionRequests::<&str>::CAPACITY as u32, &"alice")
                .is_some()
        );
    }

    const MAX_SIZE: usize = 1024 * 1024;

    fn transaction(n: u64) -> Transaction {
        Transaction::builder_localnet(Epoch(1)).build_and_seal(&tari_common_types::types::PrivateKey::from(n))
    }

    fn encoded(transaction: &Transaction) -> EncodedTransaction {
        EncodedTransaction::encode(transaction).unwrap()
    }

    fn undecodable() -> EncodedTransaction {
        EncodedTransaction::from_bytes(vec![0xff; 16])
    }

    fn request_for(transactions: &[&Transaction]) -> PendingMissingTransactionsRequest<&'static str> {
        pending(1, "alice", transactions.iter().map(|t| t.calculate_id()).collect())
    }

    #[test]
    fn the_requested_transactions_decode() {
        let (a, b) = (transaction(1), transaction(2));
        let request = request_for(&[&a, &b]);

        let decoded = request
            .decode_response(vec![encoded(&b), encoded(&a)], MAX_SIZE)
            .unwrap();

        let ids = decoded.iter().map(|t| t.calculate_id()).collect::<Vec<_>>();
        assert_eq!(ids, vec![b.calculate_id(), a.calculate_id()]);
        assert!(request.remaining_after(&decoded).is_empty());
    }

    #[test]
    fn a_partial_response_leaves_the_transactions_it_did_not_carry() {
        let (a, b, c) = (transaction(1), transaction(2), transaction(3));
        let request = request_for(&[&a, &b, &c]);

        let decoded = request.decode_response(vec![encoded(&b)], MAX_SIZE).unwrap();

        assert_eq!(
            request.remaining_after(&decoded),
            HashSet::from([a.calculate_id(), c.calculate_id()])
        );
    }

    #[test]
    fn more_transactions_than_were_requested_are_refused_before_any_is_decoded() {
        let a = transaction(1);
        let request = request_for(&[&a]);

        let err = request
            .decode_response(vec![undecodable(), undecodable()], MAX_SIZE)
            .unwrap_err();

        assert!(matches!(err, RequestedTransactionsError::TooMany { .. }), "{err}");
    }

    #[test]
    fn an_oversized_transaction_is_refused_before_it_is_decoded() {
        let a = transaction(1);
        let request = request_for(&[&a]);

        let err = request.decode_response(vec![undecodable()], 8).unwrap_err();

        assert!(matches!(err, RequestedTransactionsError::TooLarge { .. }), "{err}");
    }

    #[test]
    fn decoding_stops_at_the_first_unrequested_transaction() {
        let (a, b, c) = (transaction(1), transaction(2), transaction(3));
        let request = request_for(&[&a, &c]);

        let err = request
            .decode_response(vec![encoded(&b), undecodable()], MAX_SIZE)
            .unwrap_err();

        assert!(
            matches!(err, RequestedTransactionsError::Unrequested { transaction_id } if transaction_id == b.calculate_id()),
            "{err}"
        );
    }

    #[test]
    fn a_transaction_returned_twice_is_refused() {
        let (a, b) = (transaction(1), transaction(2));
        let request = request_for(&[&a, &b]);

        let err = request
            .decode_response(vec![encoded(&a), encoded(&a)], MAX_SIZE)
            .unwrap_err();

        assert!(matches!(err, RequestedTransactionsError::Duplicate { .. }), "{err}");
    }

    #[test]
    fn sort_unparked_local_blocks_orders_parents_before_children_by_epoch_and_height() {
        use tari_consensus_types::{ProposalCertificate, ShardGroupAccumulatedData};
        use tari_ootle_common_types::{ExtraData, NumPreshards, ProtocolVersion, ShardGroup};
        use tari_ootle_storage::consensus_models::{BlockCommands, BlockHeader};
        use tari_ootle_transaction::Network;
        use tari_sidechain::{FixedHash, QuorumDecision};
        use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

        fn proposal_at(epoch: Epoch, height: NodeHeight) -> (ProposalMessage, ProposalOrigin) {
            let shard_group = ShardGroup::all_shards(NumPreshards::P256);
            let justify = ProposalCertificate::new(
                FixedHash::zero(),
                BlockId::zero(),
                height.saturating_sub(NodeHeight(1)),
                epoch,
                shard_group,
                ProposalCertificate::genesis_signatures(),
                QuorumDecision::Accept,
            );
            let header = BlockHeader::create_unsigned(
                Network::LocalNet,
                ProtocolVersion::at(Network::LocalNet, epoch),
                BlockId::zero(),
                justify.calculate_id(),
                None,
                height,
                epoch,
                shard_group,
                RistrettoPublicKeyBytes::default(),
                FixedHash::zero(),
                &BlockCommands::empty(),
                0,
                0,
                FixedHash::zero(),
                ShardGroupAccumulatedData::default(),
                ExtraData::new(),
            )
            .unwrap();
            (
                ProposalMessage {
                    block: Block::new(header, justify, BlockCommands::empty(), None),
                    foreign_proposals: vec![],
                },
                ProposalOrigin::Live,
            )
        }

        let mut blocks = vec![
            proposal_at(Epoch(2), NodeHeight(1)),
            proposal_at(Epoch(1), NodeHeight(5)),
            proposal_at(Epoch(1), NodeHeight(4)),
        ];
        sort_unparked_local_blocks(&mut blocks);
        let order = blocks
            .iter()
            .map(|(msg, _)| (msg.block.epoch(), msg.block.height()))
            .collect::<Vec<_>>();
        assert_eq!(order, vec![
            (Epoch(1), NodeHeight(4)),
            (Epoch(1), NodeHeight(5)),
            (Epoch(2), NodeHeight(1)),
        ]);
    }

    mod catch_up_resequence {
        use std::{collections::HashMap, sync::Arc, time::Duration};

        use tari_consensus_types::{
            Decision,
            ProposalCertificate,
            ShardGroupAccumulatedData,
            ToSignatureMessage,
            ValidatorSchnorrSignature,
        };
        use tari_crypto::ristretto::RistrettoPublicKey;
        use tari_engine_types::{
            creation_check::CreationCheck,
            fees::ExhaustBurnRate,
            substate::{Substate, SubstateId},
        };
        use tari_epoch_manager::{EpochManagerError, EpochManagerEvent, PendingActivation};
        use tari_ootle_common_types::{
            ExtraData,
            NumPreshards,
            ProtocolVersion,
            ShardGroup,
            SubstateAddress,
            SubstateRequirement,
            ValidatorSetHash,
            VotePower,
        };
        use tari_ootle_storage::{
            consensus_models::{
                BlockCommands,
                BlockHeader,
                Command,
                LocalOnlyAtom,
                LockedEpoch,
                TransactionExecution,
                TransactionPool,
            },
            global::models::ValidatorNode,
        };
        use tari_ootle_transaction::Network;
        use tari_sidechain::{FixedHash, QuorumDecision};
        use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
        use tari_state_tree::ValidatorSetCommitteeProof;
        use tari_template_lib_types::{ComponentAddress, ObjectKey, crypto::RistrettoPublicKeyBytes};
        use tokio::sync::mpsc;

        use super::*;
        use crate::{
            consensus_constants::ConsensusConstants,
            leader_selection::SmoothWeightedRoundRobin,
            traits::{
                BlockTransactionExecutor,
                BlockTransactionExecutorError,
                BlockTransactionValidator,
                InboundMessaging,
                InboundMessagingError,
                OutboundMessagingError,
                PresyncStatus,
                SyncManager,
                SyncStatus,
                UncommittedChainTemplates,
                ValidatorSignatureVerifierService,
                ValidatorSignerService,
                hooks::NoopHooks,
            },
        };

        type TestAddr = RistrettoPublicKeyBytes;
        type TestStore = RocksDbStateStore<TestAddr>;

        #[derive(Clone)]
        struct MaxEpochValidator;

        impl BlockTransactionValidator for MaxEpochValidator {
            type Error = ProposalValidationError;

            fn validate_full(&self, current_epoch: Epoch, transaction: &Transaction) -> Result<(), Self::Error> {
                self.validate_epoch(current_epoch, transaction)
            }

            fn validate_epoch(&self, current_epoch: Epoch, transaction: &Transaction) -> Result<(), Self::Error> {
                if current_epoch > transaction.max_epoch() {
                    return Err(ProposalValidationError::FutureEpoch {
                        block_id: BlockId::zero(),
                        current_epoch,
                        block_epoch: transaction.max_epoch(),
                    });
                }
                Ok(())
            }
        }

        #[derive(Clone)]
        struct NoopStub;

        impl BlockTransactionExecutor<TestStore> for NoopStub {
            fn execute(
                &self,
                _transaction: &Transaction,
                _locked_epoch: LockedEpoch,
                _input_fanout: u32,
                _resolved_inputs: &HashMap<SubstateRequirement, Substate>,
                _templates: &UncommittedChainTemplates,
                _creation_check: &dyn CreationCheck,
            ) -> Result<TransactionExecution, BlockTransactionExecutorError> {
                unimplemented!()
            }
        }

        impl ValidatorSignatureVerifierService for NoopStub {}

        impl ValidatorSignerService for NoopStub {
            fn sign<M: ToSignatureMessage>(&self, _message: &M) -> ValidatorSchnorrSignature {
                unimplemented!()
            }

            fn public_key(&self) -> &RistrettoPublicKey {
                unimplemented!()
            }
        }

        impl SyncManager for NoopStub {
            type Error = HotStuffError;

            async fn check_sync(&self) -> Result<SyncStatus, Self::Error> {
                unimplemented!()
            }

            async fn sync(&mut self, _target_epoch: Option<Epoch>) -> Result<(), Self::Error> {
                unimplemented!()
            }

            async fn presync(&mut self, _shard_group: ShardGroup, _epoch: Epoch) -> Result<PresyncStatus, Self::Error> {
                unimplemented!()
            }
        }

        impl InboundMessaging for NoopStub {
            type Addr = TestAddr;

            async fn next_message(&mut self) -> Option<Result<(Self::Addr, HotstuffMessage), InboundMessagingError>> {
                unimplemented!()
            }
        }

        impl OutboundMessaging for NoopStub {
            type Addr = TestAddr;

            async fn send_self<T: Into<HotstuffMessage> + Send>(
                &mut self,
                _message: T,
            ) -> Result<(), OutboundMessagingError> {
                unimplemented!()
            }

            async fn send<T: Into<HotstuffMessage> + Send>(
                &mut self,
                _to: Self::Addr,
                _message: T,
            ) -> Result<(), OutboundMessagingError> {
                unimplemented!()
            }

            async fn multicast<T, I>(&mut self, _addresses: I, _message: T) -> Result<(), OutboundMessagingError>
            where
                I: IntoIterator<Item = Self::Addr> + Send,
                T: Into<HotstuffMessage> + Send,
            {
                unimplemented!()
            }

            async fn broadcast<T>(&mut self, _message: T) -> Result<(), OutboundMessagingError>
            where T: Into<HotstuffMessage> + Send {
                unimplemented!()
            }
        }

        impl EpochManagerReader for NoopStub {
            type Addr = TestAddr;

            fn subscribe(&self) -> broadcast::Receiver<EpochManagerEvent> {
                unimplemented!()
            }

            async fn wait_for_initial_scanning_to_complete(&self) -> Result<(), EpochManagerError> {
                unimplemented!()
            }

            async fn get_all_validator_nodes(
                &self,
                _epoch: Epoch,
            ) -> Result<Vec<ValidatorNode<Self::Addr>>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committee_info_by_validator_address(
                &self,
                _epoch: Epoch,
                _address: &Self::Addr,
            ) -> Result<CommitteeInfo, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committee_for_substate(
                &self,
                _epoch: Epoch,
                _substate_address: SubstateAddress,
            ) -> Result<Arc<Committee<Self::Addr>>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_validator_node_by_public_key(
                &self,
                _epoch: Epoch,
                _public_key: RistrettoPublicKeyBytes,
            ) -> Result<ValidatorNode<Self::Addr>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_our_validator_node(
                &self,
                _epoch: Epoch,
            ) -> Result<ValidatorNode<Self::Addr>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_local_committee_info(&self, _epoch: Epoch) -> Result<CommitteeInfo, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committee_info(
                &self,
                _epoch: Epoch,
                _shard_group: ShardGroup,
            ) -> Result<CommitteeInfo, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committee_info_for_substate(
                &self,
                _epoch: Epoch,
                _shard: SubstateAddress,
            ) -> Result<CommitteeInfo, EpochManagerError> {
                unimplemented!()
            }

            async fn current_epoch(&self) -> Result<Epoch, EpochManagerError> {
                unimplemented!()
            }

            async fn get_current_epoch_hash(&self) -> Result<FixedHash, EpochManagerError> {
                unimplemented!()
            }

            async fn get_epoch_hash(&self, _epoch: Epoch) -> Result<FixedHash, EpochManagerError> {
                unimplemented!()
            }

            async fn get_validator_set_hash(&self, _epoch: Epoch) -> Result<ValidatorSetHash, EpochManagerError> {
                unimplemented!()
            }

            async fn get_validator_set_committee_proof(
                &self,
                _epoch: Epoch,
                _shard_group: ShardGroup,
            ) -> Result<ValidatorSetCommitteeProof, EpochManagerError> {
                unimplemented!()
            }

            async fn get_num_committees(&self, _epoch: Epoch) -> Result<u32, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committee_by_shard_group(
                &self,
                _epoch: Epoch,
                _shards: ShardGroup,
            ) -> Result<Arc<Committee<Self::Addr>>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_committees_overlapping_shard_group(
                &self,
                _epoch: Epoch,
                _shard_group: ShardGroup,
            ) -> Result<HashMap<ShardGroup, Committee<Self::Addr>>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_random_committee_member(
                &self,
                _epoch: Epoch,
                _shard_group: Option<ShardGroup>,
                _excluding: HashSet<Self::Addr>,
            ) -> Result<ValidatorNode<Self::Addr>, EpochManagerError> {
                unimplemented!()
            }

            async fn lock_epoch(&self, _epoch: Epoch) -> Result<(), EpochManagerError> {
                unimplemented!()
            }

            async fn get_observed_epoch_hash(&self, _epoch: Epoch) -> Result<Option<FixedHash>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_birthday_epoch(&self) -> Result<Option<Epoch>, EpochManagerError> {
                unimplemented!()
            }

            async fn get_pending_activation(&self) -> Result<Option<PendingActivation>, EpochManagerError> {
                unimplemented!()
            }
        }

        #[derive(Clone)]
        struct TestSpec;

        impl ConsensusSpec for TestSpec {
            type Addr = TestAddr;
            type EpochManager = NoopStub;
            type Hooks = NoopHooks;
            type InboundMessaging = NoopStub;
            type LeaderStrategy = SmoothWeightedRoundRobin;
            type OutboundMessaging = NoopStub;
            type SignerService = NoopStub;
            type StateStore = TestStore;
            type SyncManager = NoopStub;
            type TransactionExecutor = NoopStub;
            type TransactionValidator = MaxEpochValidator;
        }

        fn make_catch_up_proposal(
            block_epoch: Epoch,
            shard_group: ShardGroup,
            tx_id: tari_ootle_transaction::TransactionId,
        ) -> ProposalMessage {
            let commands = BlockCommands::init(vec![Command::LocalOnly(LocalOnlyAtom {
                id: tx_id,
                decision: Decision::Commit,
                transaction_fee: 0,
                leader_fee: None,
            })])
            .unwrap();
            let justify = ProposalCertificate::new(
                FixedHash::zero(),
                BlockId::zero(),
                NodeHeight(0),
                block_epoch,
                shard_group,
                ProposalCertificate::genesis_signatures(),
                QuorumDecision::Accept,
            );
            let header = BlockHeader::create_unsigned(
                Network::LocalNet,
                ProtocolVersion::at(Network::LocalNet, block_epoch),
                BlockId::zero(),
                justify.calculate_id(),
                None,
                NodeHeight(1),
                block_epoch,
                shard_group,
                RistrettoPublicKeyBytes::default(),
                FixedHash::zero(),
                &commands,
                0,
                0,
                FixedHash::zero(),
                ShardGroupAccumulatedData::default(),
                ExtraData::new(),
            )
            .unwrap();
            ProposalMessage {
                block: Block::new(header, justify, commands, None),
                foreign_proposals: vec![],
            }
        }

        #[tokio::test]
        async fn a_catch_up_block_from_an_earlier_epoch_resequences_unpooled_transactions_against_the_block_epoch() {
            let block_epoch = Epoch(1);
            let current_epoch = Epoch(2);
            let shard_group = ShardGroup::all_shards(NumPreshards::P256);

            let dir = tempfile::tempdir().unwrap();
            let store = TestStore::open(dir.path(), DatabaseOptions::default()).unwrap();
            let pool = TransactionPool::new();
            let (tx_missing, _rx_missing) = mpsc::unbounded_channel();
            let new_transactions =
                OnReceiveNewTransaction::<TestSpec>::new(store.clone(), pool.clone(), MaxEpochValidator, tx_missing);

            let (tx_events, _rx_events) = broadcast::channel(1);
            let mut on_message_validate = OnMessageValidate::<TestSpec>::new(
                HotstuffConfig {
                    network: Network::LocalNet,
                    sidechain_id: None,
                    consensus_constants: ConsensusConstants::devnet(1),
                    state_history_length: 10,
                    pinned_checkpoint_epochs: 1,
                    state_tree_cleanup_interval: Duration::from_secs(60),
                    epoch_gc_interval: Duration::from_secs(60),
                    epoch_end_grace_period: Duration::from_secs(1),
                    epoch_end_drain_period: Duration::from_secs(1),
                    catch_up_request_timeout: Duration::from_secs(5),
                },
                store.clone(),
                NoopStub,
                NoopStub,
                NoopStub,
                tx_events.downgrade(),
            );

            let input = SubstateId::Component(ComponentAddress::from_array([1u8; ObjectKey::LENGTH]));
            let tx_rec = TransactionRecord::new(
                Transaction::builder_localnet(block_epoch)
                    .add_input(SubstateRequirement::unversioned(input))
                    .build_and_seal(&tari_common_types::types::PrivateKey::from(1u64)),
            );
            let tx_id = *tx_rec.id();

            // Persist the transaction record without pooling it (as after a state sync or pool reset).
            store.with_write_tx(|tx| tx_rec.save(tx)).unwrap();

            let proposal = make_catch_up_proposal(block_epoch, shard_group, tx_id);

            // The node's `epoch_state` has already advanced to `Epoch(2)` (`block_epoch + 1`).
            let epoch_state = EpochState {
                epoch: current_epoch,
                epoch_hash: FixedHash::zero(),
                exhaust_burn_rate: ExhaustBurnRate::new(0),
                local_committee_info: CommitteeInfo::new(
                    NumPreshards::P256,
                    1,
                    1,
                    shard_group,
                    current_epoch,
                    VotePower::of(1),
                ),
                local_committee: Arc::new(Committee::empty()),
            };

            let result = on_message_validate
                .handle_missing_transactions_local_block(
                    RistrettoPublicKeyBytes::default(),
                    &epoch_state,
                    proposal,
                    &new_transactions,
                    ProposalOrigin::CatchUp,
                )
                .await
                .unwrap();

            assert!(
                matches!(result, MessageValidationResult::Ready {
                    message: HotstuffMessage::CatchUpSyncResponse(_),
                    ..
                }),
                "expected catch-up block to be Ready, got {result:?}"
            );

            store
                .with_read_tx(|tx| {
                    assert!(
                        pool.exists(tx, &tx_id)?,
                        "unpooled transaction with max_epoch == block_epoch must be resequenced into the pool even \
                         when epoch_state is at a later epoch"
                    );
                    Ok::<_, HotStuffError>(())
                })
                .unwrap();
        }

        fn make_foreign_proposal_in_shard_group(
            epoch: Epoch,
            shard_group: ShardGroup,
        ) -> tari_ootle_storage::consensus_models::ForeignProposal {
            use tari_ootle_storage::consensus_models::{
                BlockHeaderMetadata,
                BlockPledge,
                CommandOrHash,
                CommandsCommitProof,
                ForeignProposal,
                PrunedCommand,
            };
            use tari_sidechain::{SidechainBlockCommitProof, SidechainBlockHeader};

            let metadata = BlockHeaderMetadata {
                total_leader_fee: 0,
                timestamp: 0,
                extra_data: ExtraData::new(),
                timeout_certificate_id: None,
                is_dummy: false,
            };
            let commands = vec![CommandOrHash::Pruned(PrunedCommand {
                hash: FixedHash::from([1u8; 32]),
                full_command_shard_groups: vec![],
            })];
            let commit_proof = CommandsCommitProof::new_latest(commands, SidechainBlockCommitProof {
                header: SidechainBlockHeader {
                    network: Network::LocalNet.as_byte(),
                    protocol_version: ProtocolVersion::at(Network::LocalNet, epoch).as_u32(),
                    parent_id: Default::default(),
                    justify_id: Default::default(),
                    height: 1,
                    epoch: epoch.as_u64(),
                    epoch_hash: FixedHash::zero(),
                    shard_group: tari_sidechain::ShardGroup {
                        start: shard_group.start().as_u32(),
                        end_inclusive: shard_group.end().as_u32(),
                    },
                    proposed_by: Default::default(),
                    state_merkle_root: Default::default(),
                    command_merkle_root: Default::default(),
                    transaction_merkle_root: Default::default(),
                    signature: Default::default(),
                    accumulated_data: Default::default(),
                    metadata_hash: metadata.calculate_hash(),
                },
                proof_elements: vec![],
            });
            ForeignProposal::new(commit_proof, BlockPledge::default(), metadata)
        }

        #[tokio::test]
        async fn a_foreign_proposal_message_naming_the_local_shard_group_is_discarded_before_committee_resolution() {
            let epoch = Epoch(1);
            let local_shard_group = ShardGroup::new(0, 127);

            let dir = tempfile::tempdir().unwrap();
            let store = TestStore::open(dir.path(), DatabaseOptions::default()).unwrap();
            let (tx_events, _rx_events) = broadcast::channel(1);
            // `NoopStub::get_committee_by_shard_group` panics with `unimplemented!()`, so returning `Discard` also
            // proves the message is dropped before foreign committee resolution.
            let on_message_validate = OnMessageValidate::<TestSpec>::new(
                HotstuffConfig {
                    network: Network::LocalNet,
                    sidechain_id: None,
                    consensus_constants: ConsensusConstants::devnet(1),
                    state_history_length: 10,
                    pinned_checkpoint_epochs: 1,
                    state_tree_cleanup_interval: Duration::from_secs(60),
                    epoch_gc_interval: Duration::from_secs(60),
                    epoch_end_grace_period: Duration::from_secs(1),
                    epoch_end_drain_period: Duration::from_secs(1),
                    catch_up_request_timeout: Duration::from_secs(5),
                },
                store,
                NoopStub,
                NoopStub,
                NoopStub,
                tx_events.downgrade(),
            );

            let proposal = make_foreign_proposal_in_shard_group(epoch, local_shard_group);
            let epoch_state = EpochState {
                epoch,
                epoch_hash: FixedHash::zero(),
                exhaust_burn_rate: ExhaustBurnRate::new(0),
                local_committee_info: CommitteeInfo::new(
                    NumPreshards::P256,
                    1,
                    2,
                    local_shard_group,
                    epoch,
                    VotePower::of(1),
                ),
                local_committee: Arc::new(Committee::empty()),
            };

            let result = on_message_validate
                .process_foreign_proposal(
                    &epoch_state,
                    RistrettoPublicKeyBytes::default(),
                    ForeignProposalMessage::from(proposal),
                )
                .await
                .unwrap();

            assert!(
                matches!(result, MessageValidationResult::Discard),
                "expected local-shard-group ForeignProposalMessage to be discarded, got {result:?}"
            );
        }
    }
}
