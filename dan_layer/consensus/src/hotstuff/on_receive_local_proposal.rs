//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use log::*;
use tari_dan_common_types::{
    committee::{Committee, CommitteeInfo},
    optional::Optional,
    Epoch,
    NodeHeight,
};
use tari_dan_storage::{
    consensus_models::{
        Block,
        ForeignProposalRecord,
        ForeignProposalStatus,
        HighQc,
        LastSentVote,
        NoVoteReason,
        QcId,
        SubstateRecord,
        TransactionPool,
        ValidBlock,
    },
    StateStore,
    StateStoreWriteTransaction,
};
use tari_epoch_manager::EpochManagerReader;
use tari_sidechain::QuorumDecision;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tokio::{sync::broadcast, task};

use crate::{
    hotstuff::{
        block_change_set::{BlockDecision, ProposedBlockChangeSet},
        calculate_dummy_blocks_from_justify,
        commit_proofs::generate_eviction_proofs,
        epoch_state::EpochState,
        error::HotStuffError,
        generate_epoch_checkpoint,
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
        epoch_state: &EpochState<TConsensusSpec::Addr>,
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

            self.validate_block(
                tx,
                epoch_state.epoch(),
                block,
                epoch_state.local_committee(),
                epoch_state.local_committee_info(),
            )
        })?;

        let Some(valid_block) = maybe_valid_block else {
            // Validation failed, this is already logged so we can exit here
            return Ok(false);
        };

        self.pacemaker.suspend_leader_failure().await?;

        // First validate and save the attached foreign proposals
        let is_all_foreign_proposals_valid = self.store.with_write_tx(|tx| {
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
                let mut foreign_proposal = ForeignProposalRecord::new(foreign_proposal);
                if foreign_proposal.exists(&**tx)? {
                    // This is expected behaviour, we may receive the same foreign proposal multiple times
                    debug!(
                        target: LOG_TARGET,
                        "FOREIGN PROPOSAL: Already received proposal for block {}",
                        foreign_proposal.block_id(),
                    );

                    continue;
                }

                if let Err(err) = self.on_receive_foreign_proposal.validate_and_save(
                    tx,
                    &foreign_proposal,
                    epoch_state.local_committee_info(),
                ) {
                    if let Some(err) = err.validation_error() {
                        warn!(target: LOG_TARGET, "⚠️❌ Validation failed for foreign proposal: {}", err);
                        // if a node sent us an invalid foreign proposal, we immediately reject the block
                        foreign_proposal.save(tx)?;
                        foreign_proposal.set_status(
                            tx,
                            ForeignProposalStatus::Invalid,
                            Some(valid_block.block().id()),
                        )?;
                        return Ok(false);
                    }
                    error!(target: LOG_TARGET, "Error processing foreign proposal: {}", err);
                    return Err(err);
                }
            }

            self.save_block(tx, &valid_block)?;
            info!(target: LOG_TARGET, "✅ Block {} is valid and persisted.", valid_block);
            Ok::<_, HotStuffError>(true)
        })?;

        if !is_all_foreign_proposals_valid {
            return Ok(false);
        }

        let proposer_vn = self
            .epoch_manager
            .get_validator_node_by_public_key(valid_block.epoch(), *valid_block.proposed_by())
            .await?;

        let result = self
            .process_block(
                epoch_state.epoch(),
                *epoch_state.local_committee_info(),
                epoch_state.local_committee(),
                proposer_vn.fee_claim_public_key,
                valid_block,
            )
            .await;

        match result {
            Ok(None) => {
                // The leader failure is resumed by call to update_view inside process_block
                Ok(true)
            },
            Ok(Some(NoVoteReason::AlreadyVotedAtHeight)) => {
                // We have already voted at this height, so we don't need to do anything
                // The leader failure is resumed by call to update_view inside process_block
                Ok(true)
            },
            Ok(Some(_)) => {
                if let Err(err) = self.pacemaker.resume_leader_failure().await {
                    error!(target: LOG_TARGET, "Error resuming leader failure: {}", err);
                }
                Ok(false)
            },
            Err(err) => {
                if let Err(err) = self.pacemaker.resume_leader_failure().await {
                    error!(target: LOG_TARGET, "Error resuming leader failure: {}", err);
                }
                if matches!(err, HotStuffError::ProposalValidationError(_)) {
                    self.hooks.on_block_validation_failed(&err);
                }
                Err(err)
            },
        }
    }

    async fn process_block(
        &mut self,
        current_epoch: Epoch,
        local_committee_info: CommitteeInfo,
        local_committee: &Committee<TConsensusSpec::Addr>,
        proposer_claim_public_key_bytes: RistrettoPublicKeyBytes,
        valid_block: ValidBlock,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        debug!(target: LOG_TARGET, "RECV-LOCAL-PROPOSAL - [{:?}] Starting processing block: {}", current_epoch, valid_block);

        let em_epoch = self.epoch_manager.current_epoch().await?;
        let can_propose_epoch_end = em_epoch > current_epoch;
        let is_epoch_end = valid_block.block().is_epoch_end();

        let mut on_ready_to_vote_on_local_block = self.on_ready_to_vote_on_local_block.clone();

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

        let (process_result, mut change_set) = task::spawn_blocking(move || {
            store.with_write_tx(|tx| {
                let decision = on_ready_to_vote_on_local_block.handle(
                    tx,
                    &valid_block,
                    &local_committee_info,
                    &proposer_claim_public_key_bytes,
                    can_propose_epoch_end,
                    &mut change_set,
                )?;

                Ok::<_, HotStuffError>((
                    ProcessBlockResult {
                        block_decision: decision,
                        valid_block,
                    },
                    change_set,
                ))
            })
        })
        .await??;

        // If accepted, the changeset is already saved, otherwise we log everything here for debugging
        if !process_result.block_decision.is_accept() {
            change_set.log_everything_dropped();
        }
        // Clear and reuse the memory allocated for changeset
        change_set.clear();
        self.change_set = Some(change_set);

        let no_vote = self
            .process_block_decision(local_committee_info, local_committee, process_result, is_epoch_end)
            .await?;

        Ok(no_vote)
    }

    async fn process_block_decision(
        &mut self,
        local_committee_info: CommitteeInfo,
        local_committee: &Committee<TConsensusSpec::Addr>,
        process_block_result: ProcessBlockResult,
        is_epoch_end: bool,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "process-block-decision").with_excessive_threshold(200);

        let ProcessBlockResult {
            mut block_decision,
            valid_block,
        } = process_block_result;

        let is_accept_decision = block_decision.is_accept();

        if is_accept_decision {
            let mut committed_blocks_with_evictions = block_decision.commit_blocks_with_evictions_iter().peekable();
            if committed_blocks_with_evictions.peek().is_some() {
                // Generate eviction proofs for the evicted blocks
                let qc = valid_block.justify();
                let proofs = self
                    .store
                    .with_read_tx(|tx| generate_eviction_proofs(tx, qc, committed_blocks_with_evictions))?;
                info!(target: LOG_TARGET, "🦶 Generated {} eviction proofs", proofs.len());
                for proof in proofs {
                    self.epoch_manager.add_intent_to_evict_validator(proof).await?;
                }
            }
        }

        if !block_decision.is_epoch_end() {
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
                        block_decision.high_qc.clone(),
                        decision,
                    )
                    .await?;
                }
            }
        }

        self.hooks
            .on_local_block_decide(&valid_block, block_decision.quorum_decision);
        for t in block_decision.finalized_transactions.drain(..).flatten() {
            self.hooks.on_transaction_finalized(&t.into_current_transaction_atom());
        }

        // THere should only be one committed block with end of epoch
        if let Some(eoe_block) = block_decision.take_end_of_epoch_block() {
            self.process_end_of_epoch(eoe_block, valid_block).await?;
        }

        self.propose_foreign_proposals(local_committee_info, block_decision.commit_blocks);

        // Propose quickly for the end-of-epoch chain
        if is_accept_decision && is_epoch_end {
            self.pacemaker.beat();
        }

        Ok(block_decision.no_vote_reason)
    }

    async fn process_end_of_epoch(
        &mut self,
        eoe_block: Block,
        valid_tip_block: ValidBlock,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "process-end-of-epoch");
        let prev_epoch = eoe_block.epoch();
        let next_epoch = prev_epoch + Epoch(1);

        // We're registered for the next epoch. Checkpoint and create a new genesis block.
        let num_committees = self.epoch_manager.get_num_committees(next_epoch).await?;
        let epoch_hash = self.epoch_manager.get_current_epoch_hash().await?;
        let our_vn_for_next_epoch = self.epoch_manager.get_our_validator_node(next_epoch).await.optional()?;
        let next_shard_group = our_vn_for_next_epoch.map(|vn| {
            vn.shard_key
                .to_shard_group(self.config.consensus_constants.num_preshards, num_committees)
        });
        let store = self.store.clone();
        let network = self.config.network;
        let sidechain_id = self.config.sidechain_id;
        task::spawn_blocking(move || {
            store.with_write_tx(|tx| {
                let tip_qc = valid_tip_block.justify();
                // Generate checkpoint
                let checkpoint = generate_epoch_checkpoint(&**tx, &eoe_block, tip_qc)?;
                // Technically not needed, but this will make it clear for debugging purposes if the checkpoint is
                // invalid To validate the entire proof requires the quorum threshold and the committee,
                // so we'll just skip it
                let calculated_mr = checkpoint.compute_state_merkle_root().map_err(|e| {
                    HotStuffError::InvariantError(format!("Generated an invalid epoch checkpoint: {e}"))
                })?;
                if calculated_mr != eoe_block.state_merkle_root() {
                    return Err(HotStuffError::InvariantError(format!(
                        "Epoch checkpoint state merkle root mismatch. Expected: {}, Calculated: {}, block: {}",
                        eoe_block.state_merkle_root(),
                        calculated_mr,
                        eoe_block
                    )));
                }
                checkpoint.save(tx)?;

                if let Some(next_shard_group) = next_shard_group {
                    // Create the next genesis
                    let mut genesis = Block::genesis(
                        network,
                        next_epoch,
                        epoch_hash,
                        next_shard_group,
                        *valid_tip_block.block().state_merkle_root(),
                        sidechain_id,
                    );
                    info!(target: LOG_TARGET, "⭐️ Creating new genesis block {genesis}");
                    genesis.justify().insert(tx)?;
                    genesis.insert(tx)?;
                    genesis.set_as_justified(tx, &QcId::zero())?;
                    // We'll propose using the new genesis as parent
                    genesis.as_locked_block().set(tx)?;
                    genesis.as_leaf_block().set(tx)?;
                    genesis.as_last_executed().set(tx)?;
                    genesis.as_last_voted().set(tx)?;
                    genesis.justify().as_high_qc().set(tx)?;
                }

                cleanup_epoch(tx, prev_epoch)?;

                Ok::<_, HotStuffError>(())
            })
        })
        .await??;

        self.pacemaker.set_epoch(next_epoch).await?;
        self.publish_event(HotstuffEvent::EpochChanged {
            epoch: next_epoch,
            registered_shard_group: next_shard_group,
        });

        if next_shard_group.is_some() {
            Ok(())
        } else {
            info!(
                target: LOG_TARGET,
                "💤 Our validator node is not registered for epoch {next_epoch}.",
            );

            // Exit consensus and transition to idle
            Err(HotStuffError::NotRegisteredForCurrentEpoch { epoch: next_epoch })
        }
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

    fn propose_foreign_proposals(&mut self, local_committee_info: CommitteeInfo, blocks: Vec<Block>) {
        if blocks.is_empty() || blocks.iter().all(|b| b.commands().is_empty()) {
            return;
        }

        let mut outbound_messaging = self.outbound_messaging.clone();

        task::spawn(async move {
            let _timer = TraceTimer::debug(LOG_TARGET, "propose_newly_locked_blocks").with_iterations(blocks.len());
            for block in blocks.into_iter().rev() {
                if let Err(err) = broadcast_foreign_proposal_if_required::<TConsensusSpec>(
                    &mut outbound_messaging,
                    &local_committee_info,
                    block,
                )
                .await
                {
                    error!(target: LOG_TARGET, "Error in propose_newly_locked_blocks: {}", err);
                }
            }
        });
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

        if candidate_block.justify().epoch() != current_epoch {
            return Err(ProposalValidationError::InvalidEpochInQc {
                block_id: *candidate_block.id(),
                qc_id: *candidate_block.justify().id(),
                qc_epoch: candidate_block.justify().epoch(),
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

async fn broadcast_foreign_proposal_if_required<TConsensusSpec: ConsensusSpec>(
    outbound_messaging: &mut TConsensusSpec::OutboundMessaging,
    local_committee_info: &CommitteeInfo,
    block: Block,
) -> Result<(), HotStuffError> {
    let non_local_shard_groups = block
        .commands()
        .iter()
        .flat_map(|c| {
            c.local_prepare()
                .map(|atom| (true, atom))
                .or_else(|| c.local_accept().map(|atom| (false, atom)))
        })
        .flat_map(|(is_local_prepare, atom)| {
            atom.evidence.shard_groups_iter().copied().filter(move |sg| {
                // Dont broadcast to ourselves
                if *sg == local_committee_info.shard_group() {
                    return false;
                }
                // Always broadcast LocalAccept
                if !is_local_prepare {
                    return true;
                }
                // Only broadcast LocalPrepare to input shard groups
                if atom.evidence.get(sg).is_some_and(|e| !e.inputs().is_empty()) {
                    debug!(
                        target: LOG_TARGET,
                        "🌐 FOREIGN PROPOSE: LocalPrepare({atom}) to {sg}",
                    );
                    true
                } else {
                    debug!(
                        target: LOG_TARGET,
                        "🌐 FOREIGN PROPOSE: Skipping LocalPrepare({atom}) because {sg} does not involve inputs",
                    );
                    false
                }
            })
        })
        .collect::<HashSet<_>>();

    if non_local_shard_groups.is_empty() {
        debug!(
            target: LOG_TARGET,
            "🌐 No foreign shards apply to new commit block {}",
            block,
        );
        return Ok(());
    }
    info!(
        target: LOG_TARGET,
        "🌐 FOREIGN PROPOSE: new commit block to {} foreign shard group(s). {}",
        non_local_shard_groups.len(),
        block,
    );

    for shard_group in non_local_shard_groups {
        info!(
            target: LOG_TARGET,
            "🌐 FOREIGN PROPOSE: Broadcasting commit block {} notification to shard group {}.",
            &block,
            shard_group,
        );
        // TODO: all local VNs will broadcast this. This message only needs to be published once. Perhaps we can reduce
        // this to $f+1$.
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

fn cleanup_epoch<TTx: StateStoreWriteTransaction>(tx: &mut TTx, epoch: Epoch) -> Result<(), HotStuffError> {
    // Prune all DOWNed values
    SubstateRecord::prune_downed_values(tx, epoch)?;
    ForeignProposalRecord::delete_in_epoch(tx, epoch)?;
    Ok(())
}

struct ProcessBlockResult {
    pub block_decision: BlockDecision,
    pub valid_block: ValidBlock,
}
