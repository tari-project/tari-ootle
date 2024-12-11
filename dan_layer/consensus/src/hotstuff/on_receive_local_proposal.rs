//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    mem,
};

use log::*;
use tari_dan_common_types::{
    committee::{Committee, CommitteeInfo},
    optional::Optional,
    Epoch,
    NodeHeight,
    NumPreshards,
    ShardGroup,
};
use tari_dan_storage::{
    consensus_models::{
        Block,
        HighQc,
        LastSentVote,
        QuorumCertificate,
        QuorumDecision,
        TransactionPool,
        ValidBlock,
        Vote,
    },
    StateStore,
    StateStoreWriteTransaction,
};
use tari_epoch_manager::EpochManagerReader;
use tokio::{sync::broadcast, task};

use crate::{
    hotstuff::{
        block_change_set::ProposedBlockChangeSet,
        calculate_dummy_blocks_from_justify,
        create_epoch_checkpoint,
        error::HotStuffError,
        eviction_proof::generate_eviction_proofs,
        get_next_block_height_and_leader,
        on_ready_to_vote_on_local_block::OnReadyToVoteOnLocalBlock,
        on_receive_foreign_proposal::OnReceiveForeignProposalHandler,
        pacemaker_handle::PaceMakerHandle,
        transaction_manager::ConsensusTransactionManager,
        HotstuffConfig,
        HotstuffEvent,
        ProposalValidationError,
    },
    messages::{ForeignProposalNotificationMessage, HotstuffMessage, NewViewMessage, ProposalMessage, VoteMessage},
    tracing::TraceTimer,
    traits::{
        hooks::ConsensusHooks,
        ConsensusSpec,
        OutboundMessaging,
        ValidatorSignatureService,
        VoteSignatureService,
    },
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::on_receive_local_proposal";

pub struct OnReceiveLocalProposalHandler<TConsensusSpec: ConsensusSpec> {
    config: HotstuffConfig,
    store: TConsensusSpec::StateStore,
    epoch_manager: TConsensusSpec::EpochManager,
    leader_strategy: TConsensusSpec::LeaderStrategy,
    pacemaker: PaceMakerHandle,
    on_ready_to_vote_on_local_block: OnReadyToVoteOnLocalBlock<TConsensusSpec>,
    change_set: Option<ProposedBlockChangeSet>,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
    vote_signing_service: TConsensusSpec::SignatureService,
    on_receive_foreign_proposal: OnReceiveForeignProposalHandler<TConsensusSpec>,
    tx_events: broadcast::Sender<HotstuffEvent>,
    hooks: TConsensusSpec::Hooks,
}

impl<TConsensusSpec: ConsensusSpec> OnReceiveLocalProposalHandler<TConsensusSpec> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: TConsensusSpec::StateStore,
        epoch_manager: TConsensusSpec::EpochManager,
        leader_strategy: TConsensusSpec::LeaderStrategy,
        pacemaker: PaceMakerHandle,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
        vote_signing_service: TConsensusSpec::SignatureService,
        transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
        tx_events: broadcast::Sender<HotstuffEvent>,
        transaction_manager: ConsensusTransactionManager<
            TConsensusSpec::TransactionExecutor,
            TConsensusSpec::StateStore,
        >,
        config: HotstuffConfig,
        hooks: TConsensusSpec::Hooks,
    ) -> Self {
        let local_validator_pk = vote_signing_service.public_key().clone();
        Self {
            config: config.clone(),
            store: store.clone(),
            epoch_manager: epoch_manager.clone(),
            leader_strategy,
            pacemaker: pacemaker.clone(),
            vote_signing_service,
            outbound_messaging: outbound_messaging.clone(),
            hooks,
            tx_events: tx_events.clone(),
            on_receive_foreign_proposal: OnReceiveForeignProposalHandler::new(
                store,
                epoch_manager,
                pacemaker,
                outbound_messaging,
            ),
            on_ready_to_vote_on_local_block: OnReadyToVoteOnLocalBlock::new(
                local_validator_pk,
                config,
                transaction_pool,
                tx_events,
                transaction_manager,
            ),
            change_set: None,
        }
    }

    pub async fn handle(
        &mut self,
        current_epoch: Epoch,
        local_committee_info: &CommitteeInfo,
        local_committee: &Committee<TConsensusSpec::Addr>,
        msg: ProposalMessage,
    ) -> Result<bool, HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "OnReceiveLocalProposalHandler");

        // Do not trigger leader failures while processing a proposal.
        // Leader failures will be resumed after the proposal has been processed.
        // If we vote ACCEPT for the proposal, the leader failure timer will be reset and resume, otherwise (no vote)
        // the timer will resume but not be reset.
        debug!(
            target: LOG_TARGET,
            "🔥 LOCAL PROPOSAL: block {} from {}",
            msg.block,
            msg.block.proposed_by()
        );

        let ProposalMessage {
            block,
            foreign_proposals,
        } = msg;

        let maybe_valid_block = self.store.with_read_tx(|tx| {
            if Block::has_been_justified(tx, block.id()).optional()?.unwrap_or(false) {
                info!(target: LOG_TARGET, "🧊 Block {} has already been processed", block);
                return Ok(None);
            }

            self.validate_block(tx, current_epoch, block, local_committee, local_committee_info)
        })?;

        let Some(valid_block) = maybe_valid_block else {
            // Validation failed, this is already logged so we can exit here
            return Ok(false);
        };

        self.pacemaker.suspend_leader_failure().await?;

        // First validate and save the attached foreign proposals
        let mut foreign_committees = HashMap::with_capacity(foreign_proposals.len());
        // TODO(perf): fetch all committee info in single call
        for foreign_proposal in &foreign_proposals {
            let shard_group = foreign_proposal.block.shard_group();
            if foreign_committees.contains_key(&shard_group) {
                continue;
            }
            let foreign_committee_info = self
                .epoch_manager
                .get_committee_info_by_validator_public_key(
                    foreign_proposal.block.epoch(),
                    foreign_proposal.block.proposed_by().clone(),
                )
                .await?;

            foreign_committees.insert(shard_group, foreign_committee_info);
        }

        self.store.with_write_tx(|tx| {
            // TODO: Implement guaranteed finality in the face of a non-cooperating remote shard group.
            // Suggested strategy:
            // Given a transaction that is awaiting a foreign proposal for REQUEST_FOREIGN_PROPOSAL_TIMEOUT (e.g. 50)
            // blocks
            // - Load pending transactions that are awaiting foreign proposal for >= REQUEST_FOREIGN_PROPOSAL_TIMEOUT
            // - Request foreign proposal from remote shard group [END]
            //
            // Given a transaction that is awaiting a foreign proposal for MISSING_FOREIGN_PROPOSAL_TIMEOUT (e.g. 100)
            // blocks
            // - Load pending transactions that are awaiting foreign proposal for >= MISSING_FOREIGN_PROPOSAL_TIMEOUT
            // - Set abort and ready = true
            // self.update_foreign_proposal_transactions(tx, valid_block.block())?;

            for foreign_proposal in foreign_proposals {
                if foreign_proposal.exists(&**tx)? {
                    // This is expected behaviour, we may receive the same foreign proposal multiple times
                    debug!(
                        target: LOG_TARGET,
                        "FOREIGN PROPOSAL: Already received proposal for block {}",
                        foreign_proposal.block().id(),
                    );

                    continue;
                }
                let shard_group = foreign_proposal.block().shard_group();

                self.on_receive_foreign_proposal.validate_and_save(
                    tx,
                    foreign_proposal,
                    local_committee_info,
                    foreign_committees.get(&shard_group).unwrap(),
                )?;
            }

            self.save_block(tx, &valid_block)?;
            info!(target: LOG_TARGET, "✅ Block {} is valid and persisted.", valid_block);
            Ok::<_, HotStuffError>(())
        })?;

        let result = self
            .process_block(
                current_epoch,
                *local_committee_info,
                local_committee,
                valid_block,
                foreign_committees,
            )
            .await;

        match result {
            Ok(is_accept) => Ok(is_accept),
            Err(err) => {
                if let Err(err) = self.pacemaker.resume_leader_failure().await {
                    error!(target: LOG_TARGET, "Error resuming leader failure: {:?}", err);
                }
                if matches!(err, HotStuffError::ProposalValidationError(_)) {
                    self.hooks.on_block_validation_failed(&err);
                }
                Err(err)
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn process_block(
        &mut self,
        current_epoch: Epoch,
        local_committee_info: CommitteeInfo,
        local_committee: &Committee<TConsensusSpec::Addr>,
        valid_block: ValidBlock,
        foreign_committees: HashMap<ShardGroup, CommitteeInfo>,
    ) -> Result<bool, HotStuffError> {
        let em_epoch = self.epoch_manager.current_epoch().await?;
        let can_propose_epoch_end = em_epoch > current_epoch;
        let is_epoch_end = valid_block.block().is_epoch_end();

        let mut on_ready_to_vote_on_local_block = self.on_ready_to_vote_on_local_block.clone();

        let (mut block_decision, valid_block, mut change_set) = task::spawn_blocking({
            // Reusing the change set allocated memory (pointers in the Vec types are passed onto the thread stack).
            let mut change_set = self
                .change_set
                .take()
                .map(|mut c| {
                    c.set_block(valid_block.block().as_leaf_block());
                    c
                })
                .unwrap_or_else(|| ProposedBlockChangeSet::new(valid_block.block().as_leaf_block()));

            let store = self.store.clone();

            // Task closure
            move || {
                store.with_write_tx(|tx| {
                    let decision = on_ready_to_vote_on_local_block.handle(
                        tx,
                        &valid_block,
                        &local_committee_info,
                        can_propose_epoch_end,
                        foreign_committees,
                        &mut change_set,
                    )?;

                    Ok::<_, HotStuffError>((decision, valid_block, change_set))
                })
            }
        })
        .await??;

        if !block_decision.is_accept() {
            change_set.log_everything();
        }
        // Reuse the changeset allocations after clearing it
        change_set.clear();
        self.change_set = Some(change_set);

        let is_accept_decision = block_decision.is_accept();

        if is_accept_decision && !block_decision.committed_blocks_with_evictions.is_empty() {
            let store = self.store.clone();
            let qc = valid_block.justify().clone();
            let committed_blocks_with_evictions = mem::take(&mut block_decision.committed_blocks_with_evictions);
            let proofs = task::spawn_blocking(move || {
                store.with_read_tx(|tx| generate_eviction_proofs(tx, &qc, &committed_blocks_with_evictions))
            })
            .await??;
            info!(target: LOG_TARGET, "🦶 Generated {} eviction proofs", proofs.len());
            for proof in proofs {
                self.epoch_manager.add_intent_to_evict_validator(proof).await?;
            }
        }

        if block_decision.end_of_epoch.is_none() {
            if let Some(decision) = block_decision.quorum_decision {
                let (next_height, next_leader_addr, num_skipped) = self.store.with_read_tx(|tx| {
                    get_next_block_height_and_leader(
                        tx,
                        local_committee,
                        &self.leader_strategy,
                        valid_block.id(),
                        valid_block.height(),
                    )
                })?;

                self.pacemaker
                    .update_view(valid_block.epoch(), next_height, block_decision.high_qc.block_height())
                    .await?;

                if num_skipped == 0 {
                    self.send_vote_to_leader(next_leader_addr, valid_block.block(), decision)
                        .await?;
                } else {
                    self.send_new_view_and_vote_to_leader(
                        next_height,
                        next_leader_addr,
                        valid_block.block(),
                        block_decision.high_qc,
                        decision,
                    )
                    .await?;
                }
            } else {
                self.pacemaker.resume_leader_failure().await?;
            }
        }

        self.hooks
            .on_local_block_decide(&valid_block, block_decision.quorum_decision);
        for t in block_decision.finalized_transactions.into_iter().flatten() {
            self.hooks.on_transaction_finalized(&t.into_current_transaction_atom());
        }
        self.propose_newly_locked_blocks(local_committee_info, block_decision.locked_blocks);

        if let Some(epoch) = block_decision.end_of_epoch {
            let next_epoch = epoch + Epoch(1);
            let mut registered_shard_group = None;

            // If we're registered for the next epoch. Create a new genesis block.
            if let Some(vn) = self.epoch_manager.get_our_validator_node(next_epoch).await.optional()? {
                // TODO: Change VN db to include the shard group in the ValidatorNode struct.
                let num_committees = self.epoch_manager.get_num_committees(next_epoch).await?;
                let next_shard_group = vn
                    .shard_key
                    .to_shard_group(self.config.consensus_constants.num_preshards, num_committees);
                registered_shard_group = Some(next_shard_group);
                self.store.with_write_tx(|tx| {
                    // Generate checkpoint
                    create_epoch_checkpoint(tx, epoch, local_committee_info.shard_group())?;

                    // Create the next genesis
                    let mut genesis = Block::genesis(
                        self.config.network,
                        next_epoch,
                        next_shard_group,
                        *valid_block.block().state_merkle_root(),
                        self.config.sidechain_id.clone(),
                    );
                    info!(target: LOG_TARGET, "⭐️ Creating new genesis block {genesis}");
                    genesis.justify().insert(tx)?;
                    genesis.insert(tx)?;
                    genesis.set_as_justified(tx)?;
                    // We'll propose using the new genesis as parent
                    genesis.as_locked_block().set(tx)?;
                    genesis.as_leaf_block().set(tx)?;
                    genesis.as_last_executed().set(tx)?;
                    genesis.as_last_voted().set(tx)?;
                    genesis.justify().as_high_qc().set(tx)?;

                    cleanup_epoch(tx, epoch)?;

                    Ok::<_, HotStuffError>(())
                })?;

                // TODO: We should exit consensus to sync for the epoch - when this is implemented, we will not
                // need to create the genesis, set the pacemaker, etc.
                self.pacemaker.set_epoch(next_epoch).await?;
            } else {
                info!(
                    target: LOG_TARGET,
                    "💤 Our validator node is not registered for epoch {next_epoch}.",
                )
            }
            self.publish_event(HotstuffEvent::EpochChanged {
                epoch: next_epoch,
                registered_shard_group,
            });
        }

        // Propose quickly for the end of epoch chain
        if is_accept_decision && is_epoch_end {
            self.pacemaker.beat();
        }

        Ok(is_accept_decision)
    }

    fn publish_event(&self, event: HotstuffEvent) {
        let _ignore = self.tx_events.send(event);
    }

    async fn send_vote_to_leader(
        &mut self,
        leader: &TConsensusSpec::Addr,
        block: &Block,
        decision: QuorumDecision,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "SendVoteToLeader").with_excessive_threshold(200);

        let vote = self.generate_vote_message(block, decision)?;
        info!(
            target: LOG_TARGET,
            "🔥 VOTE {:?} for block {} proposed by {} to next leader {:.4}",
            vote.decision,
            block,
            block.proposed_by(),
            leader,
        );

        let last_sent_vote = LastSentVote {
            epoch: vote.epoch,
            block_id: vote.block_id,
            block_height: block.height(),
            decision: vote.decision,
            signature: vote.signature.clone(),
        };

        self.outbound_messaging
            .send(leader.clone(), HotstuffMessage::Vote(vote))
            .await?;

        self.store.with_write_tx(|tx| last_sent_vote.set(tx))?;

        Ok(())
    }

    async fn send_new_view_and_vote_to_leader(
        &mut self,
        new_height: NodeHeight,
        leader: &TConsensusSpec::Addr,
        block: &Block,
        high_qc: HighQc,
        decision: QuorumDecision,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "send-newview-and-vote").with_excessive_threshold(200);

        let vote = self.generate_vote_message(block, decision)?;
        info!(
            target: LOG_TARGET,
            "🔥 NEWVIEW VOTE {:?} for block {} proposed by {} to next leader {:.4}",
            vote.decision,
            block,
            block.proposed_by(),
            leader,
        );

        let last_sent_vote = LastSentVote {
            epoch: vote.epoch,
            block_id: vote.block_id,
            block_height: block.height(),
            decision: vote.decision,
            signature: vote.signature.clone(),
        };

        let high_qc = if high_qc.qc_id == *block.justify().id() {
            block.justify().clone()
        } else {
            self.store.with_read_tx(|tx| high_qc.get_quorum_certificate(tx))?
        };

        let message = NewViewMessage {
            high_qc,
            new_height,
            last_vote: Some(vote),
        };

        self.outbound_messaging
            .send(leader.clone(), HotstuffMessage::NewView(message))
            .await?;

        self.store.with_write_tx(|tx| last_sent_vote.set(tx))?;

        Ok(())
    }

    fn propose_newly_locked_blocks(
        &mut self,
        local_committee_info: CommitteeInfo,
        blocks: Vec<(Block, QuorumCertificate)>,
    ) {
        if blocks.is_empty() {
            return;
        }

        task::spawn(propose_newly_locked_blocks_task::<TConsensusSpec>(
            self.epoch_manager.clone(),
            self.outbound_messaging.clone(),
            self.config.consensus_constants.num_preshards,
            local_committee_info,
            blocks,
        ));
    }

    fn generate_vote_message(&self, block: &Block, decision: QuorumDecision) -> Result<VoteMessage, HotStuffError> {
        let signature = self.vote_signing_service.sign_vote(block.id(), &decision);

        Ok(VoteMessage {
            epoch: block.epoch(),
            block_id: *block.id(),
            unverified_block_height: block.height(),
            decision,
            signature,
        })
    }

    fn save_block(
        &self,
        tx: &mut <TConsensusSpec::StateStore as StateStore>::WriteTransaction<'_>,
        valid_block: &ValidBlock,
    ) -> Result<(), HotStuffError> {
        valid_block.block().save_foreign_send_counters(tx)?;
        valid_block.block().justify().save(tx)?;
        if !valid_block.dummy_blocks().is_empty() {
            info!(target: LOG_TARGET, "Saving {} dummy block(s)", valid_block.dummy_blocks().len());
            valid_block.save_all_dummy_blocks(tx)?;
        }
        if let Err(err) = valid_block.block().save(tx) {
            error!(target: LOG_TARGET, "Error saving block: {:?}", err);
            error!(target: LOG_TARGET, "block: {}", valid_block.block());
            error!(target: LOG_TARGET, "dummy count {:?}", valid_block.dummy_blocks().len());

            for dummy in valid_block.dummy_blocks() {
                error!(target: LOG_TARGET, "dummy: {}", dummy);
            }
            let mut block = valid_block.block().clone();
            while let Some(b) = block.get_parent(&**tx).optional()? {
                block = b;
                error!(target: LOG_TARGET, "parent {}", block);
            }
            return Err(err.into());
        }

        Ok(())
    }

    fn validate_block(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        current_epoch: Epoch,
        block: Block,
        local_committee: &Committee<TConsensusSpec::Addr>,
        local_committee_info: &CommitteeInfo,
    ) -> Result<Option<ValidBlock>, HotStuffError> {
        let result =
            self.validate_local_proposed_block(tx, current_epoch, block, local_committee, local_committee_info);

        match result {
            Ok(validated) => Ok(Some(validated)),
            // Propagate this error out as sync is needed in the case where we have a valid QC but do not know the
            // block
            Err(err @ HotStuffError::ProposalValidationError(ProposalValidationError::JustifyBlockNotFound { .. })) => {
                Err(err)
            },
            // Validation errors should not cause a FAILURE state transition
            Err(HotStuffError::ProposalValidationError(err)) => {
                warn!(target: LOG_TARGET, "❌ Block failed validation: {}", err);
                // A bad block should not cause a FAILURE state transition
                Ok(None)
            },
            Err(e) => Err(e),
        }
    }

    // fn update_foreign_proposal_transactions(
    //     &self,
    //     tx: &mut <TConsensusSpec::StateStore as StateStore>::WriteTransaction<'_>,
    //     block: &Block,
    // ) -> Result<(), HotStuffError> {
    //     // TODO: Move this to consensus constants
    //     const FOREIGN_PROPOSAL_TIMEOUT: u64 = 1000;
    //     let all_proposed = ForeignProposal::get_all_proposed(
    //         &**tx,
    //         block.height().saturating_sub(NodeHeight(FOREIGN_PROPOSAL_TIMEOUT)),
    //     )?;
    //     for proposal in all_proposed {
    //         let mut has_unresolved_transactions = false;
    //
    //         let (transactions, _missing) = TransactionRecord::get_any(&**tx, &proposal.transactions)?;
    //         for transaction in transactions {
    //             if transaction.is_finalized() {
    //                 // We don't know the transaction at all, or we know it but it's not finalised.
    //                 let mut tx_rec = self
    //                     .transaction_pool
    //                     .get(&**tx, block.as_leaf_block(), transaction.id())?;
    //                 // If the transaction is still in the pool we have to check if it was at least locally prepared,
    //                 // otherwise abort it.
    //                 if tx_rec.current_stage().is_new() || tx_rec.current_stage().is_prepared() {
    //                     tx_rec.update_local_decision(tx, Decision::Abort)?;
    //                     has_unresolved_transactions = true;
    //                 }
    //             }
    //         }
    //         if !has_unresolved_transactions {
    //             proposal.delete(tx)?;
    //         }
    //     }
    //     Ok(())
    // }

    // TODO: fix
    // fn check_foreign_indexes(
    //     &self,
    //     tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
    //     num_committees: u32,
    //     local_shard: Shard,
    //     block: &Block,
    //     justify_block: &BlockId,
    // ) -> Result<(), HotStuffError> {
    //     let non_local_shards = proposer::get_non_local_shards(tx, block, num_committees, local_shard)?;
    //     let block_foreign_indexes = block.foreign_indexes();
    //     if block_foreign_indexes.len() != non_local_shards.len() {
    //         return Err(ProposalValidationError::InvalidForeignCounters {
    //             proposed_by: block.proposed_by().to_string(),
    //             hash: *block.id(),
    //             details: format!(
    //                 "Foreign indexes length ({}) does not match non-local shards length ({})",
    //                 block_foreign_indexes.len(),
    //                 non_local_shards.len()
    //             ),
    //         }
    //         .into());
    //     }
    //
    //     let mut foreign_counters = ForeignSendCounters::get_or_default(tx, justify_block)?;
    //     let mut current_shard = None;
    //     for (shard, foreign_count) in block_foreign_indexes {
    //         if let Some(current_shard) = current_shard {
    //             // Check ordering
    //             if current_shard > shard {
    //                 return Err(ProposalValidationError::InvalidForeignCounters {
    //                     proposed_by: block.proposed_by().to_string(),
    //                     hash: *block.id(),
    //                     details: format!(
    //                         "Foreign indexes are not sorted by shard. Current shard: {}, shard: {}",
    //                         current_shard, shard
    //                     ),
    //                 }
    //                 .into());
    //             }
    //         }
    //
    //         current_shard = Some(shard);
    //         // Check that each shard is correct
    //         if !non_local_shards.contains(shard) {
    //             return Err(ProposalValidationError::InvalidForeignCounters {
    //                 proposed_by: block.proposed_by().to_string(),
    //                 hash: *block.id(),
    //                 details: format!("Shard {} is not a non-local shard", shard),
    //             }
    //             .into());
    //         }
    //
    //         // Check that foreign counters are correct
    //         let expected_count = foreign_counters.increment_counter(*shard);
    //         if *foreign_count != expected_count {
    //             return Err(ProposalValidationError::InvalidForeignCounters {
    //                 proposed_by: block.proposed_by().to_string(),
    //                 hash: *block.id(),
    //                 details: format!(
    //                     "Foreign counter for shard {} is incorrect. Expected {}, got {}",
    //                     shard, expected_count, foreign_count
    //                 ),
    //             }
    //             .into());
    //         }
    //     }
    //
    //     Ok(())
    // }

    /// Perform final block validations (TODO: implement all validations)
    /// We assume at this point that initial stateless validations have been done (in inbound messages)
    #[allow(clippy::too_many_lines)]
    fn validate_local_proposed_block(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        current_epoch: Epoch,
        candidate_block: Block,
        local_committee: &Committee<TConsensusSpec::Addr>,
        _local_committee_info: &CommitteeInfo,
    ) -> Result<ValidBlock, HotStuffError> {
        if Block::has_been_justified(tx, candidate_block.id())
            .optional()?
            .unwrap_or(false)
        {
            return Err(ProposalValidationError::BlockAlreadyProcessed {
                block_id: *candidate_block.id(),
                height: candidate_block.height(),
            }
            .into());
        }

        if candidate_block.height().is_zero() {
            return Err(ProposalValidationError::MalformedBlock {
                block_id: *candidate_block.id(),
                details: "Block height is zero".to_string(),
            }
            .into());
        }

        if candidate_block.header().justify_id() != candidate_block.justify().id() {
            // Note the ID is calculated locally when the message is read from the network, therefore if this happens
            // there is a bug. This is here as a sanity check.
            return Err(ProposalValidationError::MalformedBlock {
                block_id: *candidate_block.id(),
                details: format!(
                    "BUG: justify_id ({}) in header does not match the calculated justify id ({})",
                    candidate_block.header().justify_id(),
                    candidate_block.justify().id()
                ),
            }
            .into());
        }

        if !local_committee.contains_public_key(candidate_block.proposed_by()) {
            return Err(ProposalValidationError::ValidatorNotInCommittee {
                validator: candidate_block.proposed_by().to_string(),
                details: format!(
                    "Validator node {} is not in local committee. Proposed {}",
                    candidate_block.proposed_by(),
                    candidate_block
                ),
            }
            .into());
        }

        if candidate_block.epoch() != current_epoch {
            return Err(ProposalValidationError::InvalidEpochInBlock {
                block_id: *candidate_block.id(),
                block_epoch: candidate_block.epoch(),
                current_epoch,
            }
            .into());
        }

        let justify_block = if candidate_block.justify().justifies_zero_block() {
            // The justified block is the zero block (epoch 0). However, we instead need the genesis block for the
            // epoch.
            Block::get_genesis_for_epoch(tx, candidate_block.epoch())?
        } else {
            // Load our local version of the justified block. Check that details included in the justify match
            // previously added blocks
            candidate_block.justify().get_block(tx).optional()?.ok_or_else(|| {
                // This will trigger a sync
                ProposalValidationError::JustifyBlockNotFound {
                    proposed_by: candidate_block.proposed_by().to_string(),
                    block_description: candidate_block.to_string(),
                    justify_block: candidate_block.justify().as_leaf_block(),
                }
            })?
        };

        if candidate_block.justifies_parent() && !candidate_block.parent_exists(tx)? {
            return Err(ProposalValidationError::ParentNotFound {
                proposed_by: candidate_block.proposed_by().to_string(),
                parent_id: *candidate_block.parent(),
                block_id: *candidate_block.id(),
            }
            .into());
        }

        if justify_block.height() != candidate_block.justify().block_height() {
            return Err(ProposalValidationError::JustifyBlockInvalid {
                proposed_by: candidate_block.proposed_by().to_string(),
                block_id: *candidate_block.id(),
                details: format!(
                    "Justify block height ({}) does not match justify block height ({})",
                    justify_block.height(),
                    candidate_block.justify().block_height()
                ),
            }
            .into());
        }

        if candidate_block.height() < justify_block.height() {
            return Err(ProposalValidationError::CandidateBlockNotHigherThanJustify {
                justify_block_height: justify_block.height(),
                candidate_block_height: candidate_block.height(),
            }
            .into());
        }

        // TODO: this is broken
        // self.check_foreign_indexes(
        //     tx,
        //     local_committee_info.num_committees(),
        //     local_committee_info.shard(),
        //     &candidate_block,
        //     justify_block.id(),
        // )?;

        let high_qc = HighQc::get(tx, candidate_block.epoch())?;
        // if the block parent is not the justify parent, then we have experienced a leader failure
        // and should make dummy blocks to fill in the gaps.
        if !candidate_block.justifies_parent() && candidate_block.height() > NodeHeight(1) {
            let num_dummies = candidate_block.height().as_u64() - justify_block.height().as_u64() - 1;
            info!(target: LOG_TARGET, "🔨 Creating {} dummy block(s) for block {}", num_dummies, candidate_block);

            let dummy_blocks = calculate_dummy_blocks_from_justify(
                &candidate_block,
                &justify_block,
                &self.leader_strategy,
                local_committee,
            );

            let Some(last_dummy) = dummy_blocks.last() else {
                warn!(target: LOG_TARGET, "❌ Bad proposal, does not justify parent for candidate block {}", candidate_block);
                return Err(ProposalValidationError::CandidateBlockDoesNotExtendJustify {
                    justify_block_height: justify_block.height(),
                    candidate_block_height: candidate_block.height(),
                }
                .into());
            };

            if candidate_block.parent() != last_dummy.id() {
                warn!(target: LOG_TARGET, "❌ Bad proposal, unable to find dummy blocks (last dummy: {}) for candidate block {}", last_dummy, candidate_block);
                return Err(ProposalValidationError::CandidateBlockDoesNotExtendJustify {
                    justify_block_height: justify_block.height(),
                    candidate_block_height: candidate_block.height(),
                }
                .into());
            }

            // The logic for not checking is_safe is as follows:
            // We can't without adding the dummy blocks to the DB
            // We know that justify_block is safe because we have added it to our chain
            // We know that each dummy block is built in a chain from the justify block to the candidate block
            // We know that last dummy block is the parent of candidate block
            // Therefore we know that candidate block satisfies the safeNode predicate
            return Ok(ValidBlock::with_dummy_blocks(candidate_block, dummy_blocks));
        }

        if !high_qc.block_id().is_zero() && !candidate_block.is_safe(tx)? {
            return Err(ProposalValidationError::NotSafeBlock {
                proposed_by: candidate_block.proposed_by().to_string(),
                hash: *candidate_block.id(),
            }
            .into());
        }

        Ok(ValidBlock::new(candidate_block))
    }
}

async fn propose_newly_locked_blocks_task<TConsensusSpec: ConsensusSpec>(
    epoch_manager: TConsensusSpec::EpochManager,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
    num_preshards: NumPreshards,
    local_committee_info: CommitteeInfo,
    blocks: Vec<(Block, QuorumCertificate)>,
) {
    let _timer = TraceTimer::debug(LOG_TARGET, "propose_newly_locked_blocks_task").with_iterations(blocks.len());
    if let Err(err) = propose_newly_locked_blocks_task_inner::<TConsensusSpec>(
        epoch_manager,
        outbound_messaging,
        num_preshards,
        &local_committee_info,
        blocks,
    )
    .await
    {
        error!(target: LOG_TARGET, "Error in propose_newly_locked_blocks_task: {:?}", err);
    }
}

async fn propose_newly_locked_blocks_task_inner<TConsensusSpec: ConsensusSpec>(
    epoch_manager: TConsensusSpec::EpochManager,
    mut outbound_messaging: TConsensusSpec::OutboundMessaging,
    num_preshards: NumPreshards,
    local_committee_info: &CommitteeInfo,
    blocks: Vec<(Block, QuorumCertificate)>,
) -> Result<(), HotStuffError> {
    for (block, justify_qc) in blocks.into_iter().rev() {
        broadcast_foreign_proposal_if_required::<TConsensusSpec>(
            &mut outbound_messaging,
            &epoch_manager,
            num_preshards,
            local_committee_info,
            block,
            justify_qc,
        )
        .await?;
    }
    Ok(())
}

async fn broadcast_foreign_proposal_if_required<TConsensusSpec: ConsensusSpec>(
    outbound_messaging: &mut TConsensusSpec::OutboundMessaging,
    epoch_manager: &TConsensusSpec::EpochManager,
    num_preshards: NumPreshards,
    local_committee_info: &CommitteeInfo,
    block: Block,
    justify_qc: QuorumCertificate,
) -> Result<(), HotStuffError> {
    let num_committees = epoch_manager.get_num_committees(block.epoch()).await?;

    let validator = epoch_manager.get_our_validator_node(block.epoch()).await?;
    let local_shard_group = validator.shard_key.to_shard_group(num_preshards, num_committees);
    let non_local_shard_groups = block
        .commands()
        .iter()
        .filter_map(|c| {
            c.local_prepare()
                // No need to broadcast LocalPrepare if the committee is output only
                .filter(|atom| !atom.evidence.is_committee_output_only(local_committee_info))
                .or_else(|| c.local_accept())
        })
        .flat_map(|p| p.evidence.shard_groups_iter().copied())
        .filter(|shard_group| local_shard_group != *shard_group)
        .collect::<HashSet<_>>();
    if non_local_shard_groups.is_empty() {
        debug!(
            target: LOG_TARGET,
            "🌐 No foreign shards involved for new locked block {}",
            block,
        );
        return Ok(());
    }
    info!(
        target: LOG_TARGET,
        "🌐 BROADCASTING new locked block {} to {} foreign shard groups. justify: {} ({}), parent: {}",
        block,
        non_local_shard_groups.len(),
        justify_qc.block_id(),
        justify_qc.block_height(),
        block.parent()
    );

    for shard_group in non_local_shard_groups {
        info!(
            target: LOG_TARGET,
            "🌐 FOREIGN PROPOSE: Broadcasting locked block foreign {} notification to shard group {}.",
            &block,
            shard_group,
        );
        if let Err(err) = outbound_messaging
            .broadcast(
                shard_group,
                HotstuffMessage::ForeignProposalNotification(ForeignProposalNotificationMessage {
                    block_id: *block.id(),
                    epoch: block.epoch(),
                }),
            )
            .await
        {
            error!(
                target: LOG_TARGET,
                "❌ Error broadcasting foreign proposal notification to shard group {}: {}",
                shard_group,
                err
            );
        }
    }

    Ok(())
}

fn cleanup_epoch<TTx: StateStoreWriteTransaction>(tx: &mut TTx, _epoch: Epoch) -> Result<(), HotStuffError> {
    Vote::delete_all(tx)?;
    // ForeignProposal::delete_in_epoch(tx, epoch)?;
    Ok(())
}
