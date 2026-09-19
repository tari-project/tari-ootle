//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{HashSet, VecDeque};

use log::*;
use tari_consensus_types::BlockId;
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
    consensus_models::{Block, ForeignParkedProposal, ForeignProposal, TransactionRecord},
};
use tari_ootle_transaction::TransactionId;
use tokio::sync::broadcast;

use super::config::HotstuffConfig;
use crate::{
    hotstuff::{
        CurrentView,
        HotstuffEvent,
        ProposalValidationError,
        epoch_state::EpochState,
        error::HotStuffError,
        on_receive_new_transaction::OnReceiveNewTransaction,
    },
    messages::{
        ForeignProposalMessage,
        HotstuffMessage,
        MAX_REQUESTED_TRANSACTIONS,
        MissingTransactionsRequest,
        ProposalMessage,
    },
    tracing::TraceTimer,
    traits::{ConsensusSpec, OutboundMessaging},
    validations,
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_message_validate";

pub struct OnMessageValidate<TConsensusSpec: ConsensusSpec> {
    config: HotstuffConfig,
    store: TConsensusSpec::StateStore,
    epoch_manager: TConsensusSpec::EpochManager,
    current_view: CurrentView,
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
        current_view: CurrentView,
        vote_signing_service: TConsensusSpec::SignerService,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
        tx_events: broadcast::WeakSender<HotstuffEvent>,
    ) -> Self {
        Self {
            config,
            store,
            epoch_manager,
            current_view,
            vote_signing_service,
            outbound_messaging,
            tx_events,
            active_missing_transaction_requests: MissingTransactionRequests::new(),
            current_request_id: 0,
        }
    }

    pub async fn handle(
        &mut self,
        current_height: NodeHeight,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        from: TConsensusSpec::Addr,
        msg: HotstuffMessage,
        new_transactions: &OnReceiveNewTransaction<TConsensusSpec>,
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
                self.process_local_proposal(current_height, from, epoch_state, *msg, new_transactions)
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
                self.process_catch_up_response(from, epoch_state, *msg, new_transactions)
            },
            HotstuffMessage::ForeignProposal(proposal) => {
                self.process_foreign_proposal(epoch_state, from, proposal).await
            },
            HotstuffMessage::MissingTransactionsResponse(msg) => {
                let Some(request) = self.active_missing_transaction_requests.take(msg.request_id, &from) else {
                    warn!(target: LOG_TARGET, "❓Received missing transactions (req_id = {}) from {} that we did not request. Discarding message", msg.request_id, from);
                    return Ok(MessageValidationResult::Discard);
                };

                if msg.transactions.len() > request.transactions.len() {
                    warn!(target: LOG_TARGET, "⚠️Peer {from} sent {} transaction(s) for req_id = {} but only {} were requested. Discarding message", msg.transactions.len(), msg.request_id, request.transactions.len());
                    return Ok(MessageValidationResult::Discard);
                }

                let returned_ids = msg
                    .transactions
                    .iter()
                    .map(|transaction| transaction.calculate_id())
                    .collect::<HashSet<_>>();

                if let Some(unrequested) = returned_ids.difference(&request.transactions).next() {
                    warn!(target: LOG_TARGET, "⚠️Peer {from} sent transaction {unrequested} for req_id = {} that we did not request. Discarding message", msg.request_id);
                    return Ok(MessageValidationResult::Discard);
                }

                // Each requested transaction is returned at most once, so that the count check above bounds
                // the work one response can ask for.
                if returned_ids.len() != msg.transactions.len() {
                    warn!(target: LOG_TARGET, "⚠️Peer {from} sent duplicate transactions for req_id = {}. Discarding message", msg.request_id);
                    return Ok(MessageValidationResult::Discard);
                }

                Ok(MessageValidationResult::Ready {
                    from,
                    message: HotstuffMessage::MissingTransactionsResponse(msg),
                })
            },
            HotstuffMessage::MissingTransactionsRequest(msg) => {
                if msg.transactions.len() > MAX_REQUESTED_TRANSACTIONS {
                    warn!(target: LOG_TARGET, "⚠️Peer requested more than the maximum amount of transactions. Discarding message");
                    return Ok(MessageValidationResult::Discard);
                }
                Ok(MessageValidationResult::Ready {
                    from,
                    message: HotstuffMessage::MissingTransactionsRequest(msg),
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
            .insert(request_id, to, missing_txs);
        Ok(())
    }

    fn next_request_id(&mut self) -> u32 {
        let req_id = self.current_request_id;
        self.current_request_id = self.current_request_id.wrapping_add(1);
        req_id
    }

    fn process_local_proposal(
        &mut self,
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

        if let Err(err) = self.check_local_proposal(&proposal.block, epoch_state) {
            return Ok(MessageValidationResult::Invalid {
                from,
                message: HotstuffMessage::new_proposal(proposal),
                err,
            });
        }

        self.handle_missing_transactions_local_block(from, epoch_state, proposal, new_transactions)
    }

    /// Validate a block delivered via catch-up sync. This is an ordered block import, so — unlike
    /// [`Self::process_local_proposal`] — it is intentionally NOT gated on the current view height:
    /// a node whose view has advanced beyond its stored blocks must still ingest the gap. Stateless
    /// validation and missing-transaction parking are identical to the live-proposal path.
    fn process_catch_up_response(
        &mut self,
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

        if let Err(err) = self.check_local_proposal(&proposal.block, epoch_state) {
            return Ok(MessageValidationResult::Invalid {
                from,
                message: HotstuffMessage::new_catch_up_sync_response(proposal),
                err,
            });
        }

        let missing_tx_ids = self.store.with_write_tx(|tx| {
            self.resequence_unpooled_transactions(tx, epoch_state, &proposal, new_transactions)?;
            self.check_for_missing_transactions(tx, epoch_state.local_committee_info(), &proposal)
        })?;

        if missing_tx_ids.is_empty() {
            return Ok(MessageValidationResult::Ready {
                from,
                message: HotstuffMessage::new_catch_up_sync_response(proposal),
            });
        }

        self.publish_event(HotstuffEvent::ProposedBlockParked {
            block: proposal.block.as_leaf(),
            num_missing_txs: missing_tx_ids.len(),
            num_awaiting_txs: 0,
        });

        Ok(MessageValidationResult::ParkedProposal {
            block_id: *proposal.block.id(),
            epoch: proposal.block.epoch(),
            missing_txs: missing_tx_ids,
        })
    }

    pub fn update_parked_blocks<'a, I: IntoIterator<Item = &'a TransactionId> + ExactSizeIterator>(
        &self,
        current_height: NodeHeight,
        transaction_ids: I,
    ) -> Result<(Vec<ProposalMessage>, Vec<ForeignProposalMessage>), HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "update_local_parked_blocks").with_iterations(transaction_ids.len());
        self.store.with_write_tx(|tx| {
            // TODO(perf)
            let mut unparked_blocks = Vec::new();
            let mut foreign_unparked_blocks = Vec::new();
            for transaction_id in transaction_ids {
                debug!(target: LOG_TARGET, "🔍 Checking if transaction {} unparks any blocks", transaction_id);
                if let Some((unparked_block, foreign_proposals)) =
                    tx.parked_block_remove_missing_transaction(current_height + NodeHeight(1), transaction_id)?
                {
                    info!(target: LOG_TARGET, "♻️ all transactions for local block {unparked_block} are ready for consensus");

                    self.publish_event(HotstuffEvent::ParkedBlockReady {
                        block: unparked_block.as_leaf(),
                    });

                    unparked_blocks.push(ProposalMessage {
                        block: unparked_block,
                        foreign_proposals,
                    });
                } else {
                    debug!(target: LOG_TARGET, "🔍 Transaction {} did not unpark any blocks", transaction_id);
                }

                let foreign_unparked = ForeignParkedProposal::remove_by_transaction_id(tx, transaction_id)?;
                if foreign_unparked.is_empty() {
                    debug!(target: LOG_TARGET, "🔍 Transaction {} did not unpark any foreign blocks", transaction_id);
                }else{
                    info!(target: LOG_TARGET, "♻️ all transactions for {} foreign block(s) are ready for consensus", foreign_unparked.len());
                    foreign_unparked_blocks.extend(foreign_unparked.into_iter().map(Into::into));
                }
            }
            Ok((unparked_blocks, foreign_unparked_blocks))
        })
    }

    fn check_local_proposal(
        &self,
        block: &Block,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
    ) -> Result<(), HotStuffError> {
        validations::check_local_proposal::<TConsensusSpec>(
            self.current_view.get_epoch(),
            block,
            epoch_state.local_committee(),
            epoch_state.local_committee_info(),
            &self.vote_signing_service,
            &self.config,
            epoch_state.epoch_hash(),
        )
    }

    fn check_foreign_proposal(
        &self,
        proposal: &ForeignProposal,
        committee: &Committee<TConsensusSpec::Addr>,
    ) -> Result<(), HotStuffError> {
        validations::check_foreign_proposal::<TConsensusSpec>(proposal, committee, &self.config)
    }

    fn handle_missing_transactions_local_block(
        &mut self,
        from: TConsensusSpec::Addr,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        proposal: ProposalMessage,
        new_transactions: &OnReceiveNewTransaction<TConsensusSpec>,
    ) -> Result<MessageValidationResult<TConsensusSpec::Addr>, HotStuffError> {
        let missing_tx_ids = self.store.with_write_tx(|tx| {
            self.resequence_unpooled_transactions(tx, epoch_state, &proposal, new_transactions)?;
            self.check_for_missing_transactions(tx, epoch_state.local_committee_info(), &proposal)
        })?;

        if missing_tx_ids.is_empty() {
            return Ok(MessageValidationResult::Ready {
                from,
                message: HotstuffMessage::new_proposal(proposal),
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
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        proposal: &ProposalMessage,
        new_transactions: &OnReceiveNewTransaction<TConsensusSpec>,
    ) -> Result<(), HotStuffError> {
        let ids = proposal
            .block
            .commands()
            .iter()
            .filter_map(|cmd| cmd.local_only().or_else(|| cmd.local_prepare()))
            .map(|atom| atom.id)
            .collect::<Vec<_>>();

        if ids.is_empty() {
            return Ok(());
        }

        new_transactions.resequence_known_transactions(tx, epoch_state.epoch(), ids, epoch_state.local_committee_info())
    }

    fn check_for_missing_transactions(
        &self,
        tx: &mut <TConsensusSpec::StateStore as StateStore>::WriteTransaction<'_>,
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
        let mut missing_tx_ids = TransactionRecord::get_missing(&**tx, proposal.block.all_transaction_ids())?;
        // Also park block if it has missing transactions from foreign proposals
        for proposal in &proposal.foreign_proposals {
            let foreign_missing =
                self.get_missing_transactions_for_foreign_proposal(&**tx, local_committee_info, proposal)?;
            missing_tx_ids.extend(foreign_missing);
        }

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

        tx.parked_block_insert(&proposal.block, &proposal.foreign_proposals, &missing_tx_ids)?;

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

        let Some(committee) = self
            .epoch_manager
            .get_committee_by_validator_public_key(msg.proposal.epoch(), msg.proposal.proposed_by())
            .await
            .optional()?
        else {
            warn!(
                target: LOG_TARGET,
                "❌ Foreign proposal block {} was proposed by {} who is not a registered validator for epoch {}. \
                 Discarding message.",
                msg.proposal,
                msg.proposal.proposed_by(),
                msg.proposal.epoch(),
            );
            return Ok(MessageValidationResult::Discard);
        };

        if let Err(err) = self.check_foreign_proposal(&msg.proposal, &committee) {
            return Ok(MessageValidationResult::Invalid {
                from,
                message: HotstuffMessage::ForeignProposal(msg),
                err,
            });
        }

        self.store.with_write_tx(|tx| {
            let all_involved_transactions = msg
                .proposal
                .all_transaction_ids_in_committee(epoch_state.local_committee_info());
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
                    err: ProposalValidationError::NoTransactionsInCommittee {
                        block_id,
                    }.into(),
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
    transactions: HashSet<TransactionId>,
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

    pub fn insert(&mut self, request_id: u32, to: TAddr, transactions: HashSet<TransactionId>) {
        if self.requests.len() == Self::CAPACITY {
            self.requests.pop_front();
        }
        self.requests.push_back(PendingMissingTransactionsRequest {
            request_id,
            to,
            transactions,
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn req(n: u8) -> HashSet<TransactionId> {
        [TransactionId::new([n; 32])].into_iter().collect()
    }

    #[test]
    fn a_response_from_a_peer_we_did_not_ask_is_not_matched() {
        let mut requests = MissingTransactionRequests::new();
        requests.insert(1, "alice", req(1));

        assert!(requests.take(1, &"bob").is_none());
        assert!(requests.take(1, &"alice").is_some());
    }

    #[test]
    fn the_same_id_to_two_peers_matches_each_peer_once() {
        let mut requests = MissingTransactionRequests::new();
        requests.insert(1, "alice", req(1));
        requests.insert(1, "bob", req(2));

        assert_eq!(requests.take(1, &"bob").unwrap().transactions, req(2));
        assert_eq!(requests.take(1, &"alice").unwrap().transactions, req(1));
        assert!(requests.take(1, &"alice").is_none());
    }

    #[test]
    fn the_oldest_request_is_evicted_once_capacity_is_reached() {
        let mut requests = MissingTransactionRequests::new();
        for i in 0..=MissingTransactionRequests::<&str>::CAPACITY {
            requests.insert(i as u32, "alice", req(0));
        }

        assert!(requests.take(0, &"alice").is_none());
        assert!(
            requests
                .take(MissingTransactionRequests::<&str>::CAPACITY as u32, &"alice")
                .is_some()
        );
    }
}
