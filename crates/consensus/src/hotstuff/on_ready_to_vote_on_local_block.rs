//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::{BlockId, LastVoted, PcId, ProposalCertificate};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_ootle_common_types::{
    Epoch,
    ShardGroup,
    ValidatorSetHash,
    committee::{Committee, CommitteeInfo},
    optional::Optional,
};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{
        Block,
        BlockDiff,
        BookkeepingModel,
        Command,
        CommittedBlockProof,
        ForeignProposalAtom,
        ForeignProposalRecord,
        ForeignProposalStatus,
        NoVoteReason,
        ProposalOrigin,
        SubstateRecord,
        TransactionPool,
        TransactionPoolRecord,
        TransactionRecord,
        ValidBlock,
        ValidatorStatsUpdate,
        index_committed_block_state_versions,
        index_substate_down_proofs,
    },
};
use tari_sidechain::{FixedHash, QuorumDecision};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tokio::sync::broadcast;

use crate::{
    hotstuff::{
        HotstuffConfig,
        ProposalValidationError,
        block_change_set::{BlockDecision, ProposedBlockChangeSet},
        command_sequencer::{BlockTotals, block_state_root},
        commit_proofs::{committed_block_commit_proof, generate_block_commit_proof},
        error::HotStuffError,
        event::HotstuffEvent,
        exceeds_block_foreign_proposal_bytes,
        exhaust_burn_rate::resolve_epoch_exhaust_burn_rate,
        foreign_proposal_processor::process_foreign_block,
        local_command_validator::LocalCommandValidator,
        process_newly_justified_block,
        substate_store::{PendingSubstateStore, ShardedStateTree},
        transaction_manager::{ConsensusTransactionManager, TransactionLockConflicts, UncommittedTemplateStore},
    },
    tracing::TraceTimer,
    traits::{BlockStore, BlockTransactionExecutor, CertificateStore},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_ready_to_vote_on_local_block";

/// This node's own view of the epoch after the one a block is in, against which an end-of-epoch block is ratified.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalNextEpochView {
    /// The next epoch's boundary hash, or `None` if this node's oracle has not observed it.
    pub epoch_hash: Option<FixedHash>,
    /// The next epoch's validator set hash, or `None` if this node has not assigned the next epoch's committees.
    pub validator_set_hash: Option<ValidatorSetHash>,
}

#[derive(Debug, Clone)]
pub struct OnReadyToVoteOnLocalBlock<TStateStore: StateStore, TExecutor: BlockTransactionExecutor<TStateStore>> {
    local_validator_pk: RistrettoPublicKey,
    config: HotstuffConfig,
    transaction_pool: TransactionPool<TStateStore>,
    tx_events: broadcast::WeakSender<HotstuffEvent>,
    transaction_manager: ConsensusTransactionManager<TExecutor, TStateStore>,
}

impl<TStateStore, TExecutor> OnReadyToVoteOnLocalBlock<TStateStore, TExecutor>
where
    TStateStore: StateStore,
    TExecutor: BlockTransactionExecutor<TStateStore>,
{
    pub fn new(
        local_validator_pk: RistrettoPublicKey,
        config: HotstuffConfig,
        transaction_pool: TransactionPool<TStateStore>,
        tx_events: broadcast::WeakSender<HotstuffEvent>,
        transaction_manager: ConsensusTransactionManager<TExecutor, TStateStore>,
    ) -> Self {
        Self {
            local_validator_pk,
            config,
            transaction_pool,
            tx_events,
            transaction_manager,
        }
    }

    pub fn uncommitted_templates(&self) -> &UncommittedTemplateStore {
        self.transaction_manager.uncommitted_templates()
    }

    pub fn handle(
        &mut self,
        tx: &mut TStateStore::WriteTransaction<'_>,
        valid_block: &ValidBlock,
        local_committee_info: &CommitteeInfo,
        local_committee: &Committee<TStateStore::Addr>,
        proposer_claim_public_key_bytes: &RistrettoPublicKeyBytes,
        can_propose_epoch_end: bool,
        // Used to ratify what an end-of-epoch block commits the next epoch to before voting. Where this node does
        // not know a value, it cannot ratify it and abstains.
        local_next_epoch: LocalNextEpochView,
        // True if the local oracle reports a next epoch with a different number of committees. That permits aborting
        // a transaction from LocalPrepared (see `abort_local_prepared_for_epoch_end`) and requires the end-of-epoch
        // block to leave none there (see `check_epoch_end_drained`).
        next_epoch_changes_layout: bool,
        origin: ProposalOrigin,
        change_set: &mut ProposedBlockChangeSet,
    ) -> Result<BlockDecision, HotStuffError> {
        let _timer =
            TraceTimer::info(LOG_TARGET, "Decide on local block").with_iterations(valid_block.block().commands().len());
        debug!(
            target: LOG_TARGET,
            "🔥 LOCAL PROPOSAL READY: {}",
            valid_block,
        );

        let block_qc = valid_block.block().justify();
        let block_qc_id = block_qc.calculate_id();
        // The QC is valid, update high QC - Regardless if we accept the proposal commands.
        // Update high TC
        let maybe_high_tc = valid_block
            .block()
            .timeout_certificate()
            .map(|tc| tc.update_highest(tx))
            .transpose()?;

        let mut commit_blocks = Vec::new();
        let mut finalized_transactions = Vec::new();
        // Update nodes
        let high_qc = valid_block.block().update_nodes(
            tx,
            |tx, _prev_locked, block, _justify_qc| self.on_lock_block(tx, block),
            |tx, mut commit_block| {
                let committed = self.on_commit(
                    tx,
                    local_committee_info,
                    local_committee,
                    block_qc,
                    &block_qc_id,
                    &commit_block,
                )?;
                // NOTE: update the commit QC in the local copy so that foreign proposals can obtain the commit QC
                // on_commit already sets the persisted commit_qc for the block
                commit_block.set_commit_qc(block_qc_id);
                if !commit_block.is_dummy() {
                    commit_blocks.push(commit_block);
                }
                finalized_transactions.extend(committed);
                Ok(())
            },
        )?;

        // Process newly justified block
        let mut justified_block = Block::get_justified_block(&**tx, valid_block.justify(), valid_block.epoch())?;
        // This comes before decide so that all evidence can be in place before LocalPrepare and LocalAccept
        let processed_blocks =
            process_newly_justified_block(&**tx, &justified_block, block_qc_id, local_committee_info, change_set)?;

        // A block at a height this node has already voted at may still be the one the committee certifies, and
        // committing it applies its change set. It is therefore decided on, and its change set saved, whether or not
        // this node may vote for it. Blocks first imported through catch-up have already been certified by the
        // committee and are never voted on by the caller, so their decision is reported even at or below
        // `last_voted`.
        let may_vote = origin.is_catch_up() || self.should_vote(&**tx, valid_block.block())?;
        let parent = valid_block.block().get_parent(&**tx)?;
        self.decide_what_to_vote(
            &**tx,
            &parent,
            valid_block.block(),
            local_committee_info,
            proposer_claim_public_key_bytes,
            can_propose_epoch_end,
            local_next_epoch,
            next_epoch_changes_layout,
            change_set,
        )?;

        if change_set.is_accept() {
            // A justified block and its walked ancestors are stamped only by an accepted proposal, whose change set
            // carries their evidence. A later justification walks back to the first stamped ancestor.
            for mut block in processed_blocks {
                block.add_justify_qc(tx, &block_qc_id)?;
            }
            justified_block.add_justify_qc(tx, &block_qc_id)?;
            info!(
                target: LOG_TARGET,
                "✅ Saving changeset: {}",
                change_set
            );
            change_set.save(tx)?;
            self.transaction_manager
                .uncommitted_templates()
                .record(change_set.block(), change_set.local_substate_changes());
            if let Some(reason) = change_set.abstention() &&
                let Err(err) = tx.diagnostics_add_no_vote(*valid_block.id(), reason.clone())
            {
                error!(target: LOG_TARGET, "Failed to save no vote reason: {err}");
            }
        } else {
            warn!(
                target: LOG_TARGET,
                "❌ NOT voting on block {}. Change set: {}",
                valid_block.block(),
                change_set,
            );
        }

        let (local_decision, no_vote_reason) = if !may_vote {
            (None, Some(NoVoteReason::AlreadyVotedAtHeight))
        } else if let Some(reason) = change_set.no_vote_reason() {
            (None, Some(reason.clone()))
        } else if let Some(reason) = change_set.abstention() {
            (None, Some(reason.clone()))
        } else {
            (change_set.quorum_decision(), None)
        };

        Ok(BlockDecision {
            local_decision,
            commit_blocks,
            finalized_transactions,
            high_pc: high_qc,
            new_high_tc: maybe_high_tc,
            no_vote_reason,
        })
    }

    /// if b_new .height > vheight && (b_new extends b_lock || b_new .justify.node.height > b_lock .height)
    ///
    /// If we have not previously voted on this block and the node extends the current locked node, then we vote
    fn should_vote<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        block: &Block,
    ) -> Result<bool, ProposalValidationError> {
        let Some(last_voted) = LastVoted::get(tx, block.epoch()).optional()? else {
            // Never voted, then validated.block.height() > last_voted.height (0)
            return Ok(true);
        };

        // if b_new .height > vheight And ...
        if block.height() <= last_voted.height() {
            info!(
                target: LOG_TARGET,
                "❌ NOT voting on block {}. Block height is not greater than last voted height {}",
                block,
                last_voted.height(),
            );
            return Ok(false);
        }

        Ok(true)
    }

    #[allow(clippy::too_many_lines)]
    fn decide_what_to_vote<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        parent: &Block,
        block: &Block,
        local_committee_info: &CommitteeInfo,
        proposer_claim_public_key_bytes: &RistrettoPublicKeyBytes,
        can_propose_epoch_end: bool,
        local_next_epoch: LocalNextEpochView,
        next_epoch_changes_layout: bool,
        proposed_block_change_set: &mut ProposedBlockChangeSet,
    ) -> Result<(), HotStuffError> {
        if let Some(reason) = check_no_commands_after_epoch_end(tx, parent, block)? {
            warn!(target: LOG_TARGET, "❌ NO VOTE: {reason} (block {block})");
            proposed_block_change_set.set_no_vote(reason);
            return Ok(());
        }

        // Reject (no-vote) a block whose total transaction execution weight exceeds the network cap,
        // before executing any of its commands. This bounds how long a replica can be made to spend
        // executing a single block, so a misbehaving leader cannot push replicas past the block time by
        // packing more than the propose-time budget. A block with a single transaction command is exempt
        // so an individually-heavy transaction stays committable (its static weight also over-estimates
        // its execution). CONSENSUS RULE: `max_block_validation_weight` must be uniform network-wide.
        let max_validation_weight = self.config.consensus_constants.max_block_validation_weight;
        let mut block_execution_weight = 0u64;
        let mut num_transaction_commands = 0usize;
        for cmd in block.commands() {
            let Some(atom) = cmd.transaction() else {
                continue;
            };
            num_transaction_commands += 1;
            let weight = atom
                .get_transaction(tx)?
                .transaction()
                .calculate_transaction_weight()
                .as_u64();
            block_execution_weight =
                block_execution_weight.saturating_add(weight.saturating_mul(cmd.execution_weight_percent()) / 100);
        }
        if exceeds_block_validation_weight(block_execution_weight, num_transaction_commands, max_validation_weight) {
            let reason = NoVoteReason::BlockWeightExceeded {
                total_weight: block_execution_weight,
                max_weight: max_validation_weight,
            };
            warn!(target: LOG_TARGET, "❌ NO VOTE: {reason}");
            proposed_block_change_set.set_no_vote(reason);
            return Ok(());
        }

        // Store used for transactions that have inputs without specific versions.
        // It lives through the entire block so multiple transactions can be sequenced together in the same block.
        // CONSENSUS RULE: a block may not have this shard group load more than `max_block_input_load_bytes` of input
        // substates for its transactions. The budget is checked as each transaction's inputs load, so evaluation
        // stops (no-vote) at the first input past it rather than after the whole block has been loaded.
        let mut substate_store =
            PendingSubstateStore::init(tx, block.as_leaf(), self.config.consensus_constants.num_preshards)?
                .with_input_load_budget(self.config.consensus_constants.max_block_input_load_bytes);
        let mut totals = BlockTotals::extending(parent.header().total_accumulated_exhaust_burn());
        let max_validation_execution_points = self.config.consensus_constants.max_block_validation_execution_points;
        let mut block_execution_points = 0u64;
        let num_foreign_proposals = block.all_foreign_proposals().count();
        let mut foreign_proposal_bytes = 0usize;

        let command_validator = LocalCommandValidator::new(&self.transaction_manager, &self.local_validator_pk);
        for cmd in block.commands() {
            match cmd {
                Command::LocalOnly(atom) => {
                    if let Some(reason) = command_validator.evaluate_local_only_command(
                        tx,
                        block,
                        atom,
                        local_committee_info,
                        &mut substate_store,
                        proposed_block_change_set,
                        &mut totals,
                    )? {
                        proposed_block_change_set.set_no_vote(reason);
                        return Ok(());
                    }
                },
                Command::LocalPrepare(atom) => {
                    if let Some(reason) = command_validator.evaluate_local_prepare_command(
                        tx,
                        block,
                        atom,
                        local_committee_info,
                        &mut substate_store,
                        proposed_block_change_set,
                        &mut totals,
                    )? {
                        proposed_block_change_set.set_no_vote(reason);
                        return Ok(());
                    }
                },
                Command::LocalAccept(atom) => {
                    if let Some(reason) = command_validator.evaluate_local_accept_command(
                        tx,
                        block,
                        atom,
                        local_committee_info,
                        next_epoch_changes_layout,
                        &mut substate_store,
                        proposed_block_change_set,
                        &mut totals,
                    )? {
                        proposed_block_change_set.set_no_vote(reason);
                        return Ok(());
                    }
                },
                Command::AllAccept(atom) => {
                    if let Some(reason) = command_validator.evaluate_all_accept_command(
                        tx,
                        block,
                        atom,
                        local_committee_info,
                        &mut substate_store,
                        proposed_block_change_set,
                        &mut totals,
                    )? {
                        proposed_block_change_set.set_no_vote(reason);
                        return Ok(());
                    }
                },
                Command::SomeAccept(atom) => {
                    if let Some(reason) = command_validator.evaluate_some_accept_command(
                        tx,
                        block,
                        atom,
                        local_committee_info,
                        &mut substate_store,
                        proposed_block_change_set,
                        &mut totals,
                    )? {
                        proposed_block_change_set.set_no_vote(reason);
                        return Ok(());
                    }
                },
                Command::ForeignProposal(fp_atom) => {
                    if let Some(reason) = self.evaluate_foreign_proposal_command(
                        tx,
                        block,
                        fp_atom,
                        local_committee_info,
                        fp_atom.shard_group,
                        num_foreign_proposals,
                        &mut foreign_proposal_bytes,
                        &mut substate_store,
                        proposed_block_change_set,
                    )? {
                        proposed_block_change_set.set_no_vote(reason);
                        return Ok(());
                    }

                    continue;
                },
                // The values an EndEpoch commits the next epoch to are ratified against this node's own view of the
                // next epoch. Where that view does not confirm a value, the node abstains: it keeps the block's change
                // set and withholds its vote. The committee may commit the block on the votes of members that do
                // confirm it, and this node then commits it too, which requires its change set.
                Command::EndEpoch(atom) => {
                    if !can_propose_epoch_end {
                        warn!(
                            target: LOG_TARGET,
                            "❌ EpochEvent::End command received for block {} but it is not the next epoch",
                            block.id(),
                        );
                        proposed_block_change_set.abstain(NoVoteReason::NotEndOfEpoch);
                    }
                    if block.commands().len() > 1 {
                        warn!(
                            target: LOG_TARGET,
                            "❌ EpochEvent::End command in block {} but block contains other commands",
                            block.id()
                        );
                        proposed_block_change_set.set_no_vote(NoVoteReason::EndOfEpochWithOtherCommands);
                        return Ok(());
                    }
                    if let Some(reason) =
                        check_epoch_end_drained(tx, &block.justify().calculate_block_id(), next_epoch_changes_layout)?
                    {
                        warn!(target: LOG_TARGET, "❌ NO VOTE: {reason} (block {block})");
                        proposed_block_change_set.set_no_vote(reason);
                        return Ok(());
                    }

                    // Ratify the next epoch's hash against our own (lagged, reorg-stable) oracle. Voting
                    // for the EOE block is how the committee agrees on the next epoch's boundary hash;
                    // by gating on the LOCAL oracle (not on the majority signal that may have set
                    // can_propose_epoch_end above), a node never lends quorum to a hash it has not
                    // itself observed. If our oracle has not crossed the boundary, abstain until it has.
                    match local_next_epoch.epoch_hash {
                        Some(local) if local == *atom.next_epoch_hash() => {},
                        Some(local) => {
                            warn!(
                                target: LOG_TARGET,
                                "❌ NO VOTE: EndEpoch in block {} proposes next-epoch hash {} but our oracle has {}",
                                block.id(),
                                atom.next_epoch_hash(),
                                local,
                            );
                            proposed_block_change_set.abstain(NoVoteReason::EndOfEpochHashMismatch {
                                local,
                                proposed: *atom.next_epoch_hash(),
                            });
                        },
                        None => {
                            warn!(
                                target: LOG_TARGET,
                                "❌ NO VOTE: EndEpoch in block {} but our oracle has not yet observed the next epoch boundary block",
                                block.id(),
                            );
                            proposed_block_change_set.abstain(NoVoteReason::EndOfEpochHashNotObserved);
                        },
                    }

                    // Ratify the rate the next epoch opens at the same way, and against this node's own
                    // resolution rather than the proposer's. The rate is fixed for the whole of an epoch by
                    // the block that opens it, so this vote is the only point at which the committee agrees
                    // on it; a leader that resolved something else gets no quorum for it.
                    let local_rate =
                        resolve_epoch_exhaust_burn_rate(tx, self.config.network, block.epoch() + Epoch(1))?;
                    match block.next_epoch_exhaust_burn_rate() {
                        Some(proposed) if proposed == local_rate => {},
                        Some(proposed) => {
                            warn!(
                                target: LOG_TARGET,
                                "❌ NO VOTE: EndEpoch in block {} opens the next epoch at {}bps but we resolve {}bps",
                                block.id(),
                                proposed.as_bps(),
                                local_rate.as_bps(),
                            );
                            proposed_block_change_set.abstain(NoVoteReason::EndOfEpochBurnRateMismatch {
                                local_bps: local_rate.as_bps(),
                                proposed_bps: proposed.as_bps(),
                            });
                        },
                        None => {
                            warn!(
                                target: LOG_TARGET,
                                "❌ NO VOTE: EndEpoch in block {} does not name the exhaust burn rate for the next epoch",
                                block.id(),
                            );
                            proposed_block_change_set.set_no_vote(NoVoteReason::EndOfEpochBurnRateMissing);
                            return Ok(());
                        },
                    }

                    // Ratify the next epoch's validator set the same way. The quorum committing this block is what
                    // hands the chain over to the next epoch's committees, so a node only lends its vote to a set
                    // it has assigned itself.
                    if let Some(reason) = check_next_epoch_validator_set(
                        local_next_epoch.validator_set_hash,
                        block.next_epoch_validator_set_hash(),
                    ) {
                        warn!(target: LOG_TARGET, "❌ NO VOTE: {reason} (block {})", block.id());
                        if matches!(reason, NoVoteReason::EndOfEpochValidatorSetMissing) {
                            proposed_block_change_set.set_no_vote(reason);
                            return Ok(());
                        }
                        proposed_block_change_set.abstain(reason);
                    }

                    continue;
                },
            }

            // CONSENSUS RULE: a block may not execute more than `max_block_validation_execution_points`. The
            // total covers WASM metering and native crypto verification alike — both are real CPU time on every
            // replica, and bounding only the WASM half would leave stealth and confidential verification
            // ungoverned at the block level. It is accumulated as commands execute and the first command that
            // pushes it over the limit stops block execution (no-vote), bounding the compute a misbehaving
            // leader can extract from replicas. Both halves are deterministic — metering is, and the native
            // price is a pure function of the declared statement — so every replica computes identical totals
            // and stops at the same command. Commands executed for this block have an execution in the change set;
            // finalization commands (AllAccept/SomeAccept) reuse a prior block's execution and add nothing.
            if let Some(atom) = cmd.transaction() &&
                let Some(execution) = proposed_block_change_set.transaction_execution(atom.id())
            {
                block_execution_points =
                    block_execution_points.saturating_add(execution.result().total_execution_points());
                if block_execution_points > max_validation_execution_points {
                    let reason = NoVoteReason::BlockExecutionPointsExceeded {
                        total_points: block_execution_points,
                        max_points: max_validation_execution_points,
                    };
                    warn!(target: LOG_TARGET, "❌ NO VOTE: {reason}");
                    proposed_block_change_set.set_no_vote(reason);
                    return Ok(());
                }
            }
        }

        // CONSENSUS RULE: a block's writes to a shard form that shard's next state version, which state sync
        // transfers and commits whole, so a block may not put up more than
        // `max_block_validation_shard_output_bytes` in any one shard.
        let max_shard_output_bytes = self.config.consensus_constants.max_block_validation_shard_output_bytes;
        let shard_output_bytes = substate_store.max_shard_output_bytes();
        if shard_output_bytes > max_shard_output_bytes {
            let reason = NoVoteReason::BlockShardOutputBytesExceeded {
                total_bytes: shard_output_bytes,
                max_bytes: max_shard_output_bytes,
            };
            warn!(target: LOG_TARGET, "❌ NO VOTE: {reason}");
            proposed_block_change_set.set_no_vote(reason);
            return Ok(());
        }

        // CONSENSUS RULE: every foreign shard group receives a subset of the inputs this block pledges, inline in a
        // single foreign proposal, so the block may not pledge more than `max_block_pledge_bytes`. Each input is
        // counted once however many commands pledge it, so the total does not depend on the order they are
        // evaluated in.
        let max_pledge_bytes = self.config.consensus_constants.max_block_pledge_bytes;
        let pledge_bytes = substate_store.pledge_bytes();
        if pledge_bytes > max_pledge_bytes {
            let reason = NoVoteReason::BlockPledgeBytesExceeded {
                total_bytes: pledge_bytes,
                max_bytes: max_pledge_bytes,
            };
            warn!(target: LOG_TARGET, "❌ NO VOTE: {reason}");
            proposed_block_change_set.set_no_vote(reason);
            return Ok(());
        }

        if totals.leader_fee != block.total_leader_fee() {
            warn!(
                target: LOG_TARGET,
                "❌ Leader fee disagreement for block {}. Leader proposed {}, we calculated {}",
                block,
                block.total_leader_fee(),
                totals.leader_fee
            );
            proposed_block_change_set.set_no_vote(NoVoteReason::TotalLeaderFeeDisagreement);
            return Ok(());
        }

        if totals.exhaust_burn != block.header().total_accumulated_exhaust_burn() {
            warn!(
                target: LOG_TARGET,
                "❌ Exhaust burn disagreement for block {}. Leader proposed {}, we calculated {}",
                block,
                block.header().total_accumulated_exhaust_burn(),
                totals.exhaust_burn
            );
            proposed_block_change_set.set_no_vote(NoVoteReason::TotalExhaustBurnDisagreement);
            return Ok(());
        }

        let (expected_merkle_root, tree_diffs) = block_state_root(
            tx,
            local_committee_info,
            block.parent(),
            &mut substate_store,
            proposer_claim_public_key_bytes,
            totals.leader_fee,
            block.network(),
            block.epoch(),
        )?;
        if expected_merkle_root != *block.state_merkle_root() {
            warn!(
                target: LOG_TARGET,
                "❌ State Merkle root disagreement for block {}. Leader proposed {}, we calculated {}",
                block,
                block.state_merkle_root(),
                expected_merkle_root
            );
            let (diff, locks) = substate_store.into_parts();
            let diff = BlockDiff::new(*block.id(), diff);
            proposed_block_change_set
                .set_no_vote(NoVoteReason::StateMerkleRootMismatch)
                // These are set for debugging purposes but aren't actually committed
                .set_block_diff_to_commit(diff.into_filtered(local_committee_info.shard_group()))
                .set_substate_locks(locks);
            return Ok(());
        }

        let (diff, locks) = substate_store.into_parts();
        let diff = BlockDiff::new(*block.id(), diff);
        proposed_block_change_set
            .set_block_diff_to_commit(diff)
            .set_state_tree_diffs(tree_diffs)
            .set_substate_locks(locks)
            .set_quorum_decision(QuorumDecision::Accept);

        Ok(())
    }

    fn evaluate_foreign_proposal_command<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        local_block: &Block,
        fp_atom: &ForeignProposalAtom,
        local_committee_info: &CommitteeInfo,
        foreign_shard_group: ShardGroup,
        num_foreign_proposals: usize,
        foreign_proposal_bytes: &mut usize,
        substate_store: &mut PendingSubstateStore<TTx>,
        proposed_block_change_set: &mut ProposedBlockChangeSet,
    ) -> Result<Option<NoVoteReason>, HotStuffError> {
        if proposed_block_change_set
            .proposed_foreign_proposals()
            .contains(&fp_atom.block_id)
        {
            warn!(
                target: LOG_TARGET,
                "❌ NO VOTE: Foreign proposal {block_id} has already been proposed in this block.",
                block_id = fp_atom.block_id,
            );
            return Ok(Some(NoVoteReason::ForeignProposalAlreadyProposed));
        }

        let Some(fp) = fp_atom.get_proposal(tx).optional()? else {
            warn!(
                target: LOG_TARGET,
                "❌ NO VOTE: Foreign proposal {block_id} has not been received.",
                block_id = fp_atom.block_id,
            );
            return Ok(Some(NoVoteReason::ForeignProposalNotReceived));
        };

        // CONSENSUS RULE: the block travels with its foreign proposals in one message, so together they may not exceed
        // `max_block_validation_foreign_proposal_bytes`. Summed as each is evaluated against the block's full count,
        // so the verdict does not depend on the order they are evaluated in.
        *foreign_proposal_bytes = foreign_proposal_bytes.saturating_add(tari_bor::encoded_len(fp.proposal()));
        let max_foreign_proposal_bytes = self
            .config
            .consensus_constants
            .max_block_validation_foreign_proposal_bytes;
        if exceeds_block_foreign_proposal_bytes(
            *foreign_proposal_bytes,
            num_foreign_proposals,
            max_foreign_proposal_bytes,
        ) {
            let reason = NoVoteReason::BlockForeignProposalBytesExceeded {
                num_proposals: num_foreign_proposals,
                total_bytes: *foreign_proposal_bytes,
                max_bytes: max_foreign_proposal_bytes,
            };
            warn!(target: LOG_TARGET, "❌ NO VOTE: {reason}");
            return Ok(Some(reason));
        }

        // CONSENSUS RULE: the stored record must be one an honest proposer would select (see
        // `check_foreign_proposal_record`)
        if let Some(reason) = check_foreign_proposal_record(fp_atom, &fp) {
            warn!(
                target: LOG_TARGET,
                "❌ NO VOTE: Foreign proposal {block_id} (status={status}, atom_shard_group={atom_sg}, proposal_shard_group={proposal_sg}): {reason}",
                block_id = fp_atom.block_id,
                status = fp.status(),
                atom_sg = fp_atom.shard_group,
                proposal_sg = fp.proposal().shard_group_unchecked(),
            );
            return Ok(Some(reason));
        }

        if let Err(err) = process_foreign_block(
            tx,
            fp.proposal(),
            local_committee_info,
            substate_store,
            proposed_block_change_set,
        ) {
            // TODO: split validation errors from HotStuff errors so that we can selectively crash or not vote
            warn!(
                target: LOG_TARGET,
                "❌ NO VOTE: Failed to process foreign proposal {foreign_block_id} from {shard_group} for local block {block} Error: {error}",
                block = local_block,
                foreign_block_id = fp_atom.block_id,
                error = err,
                shard_group = foreign_shard_group,
            );
            return Ok(Some(NoVoteReason::ForeignProposalProcessingFailed));
        }

        proposed_block_change_set.set_foreign_proposal_proposed_in(fp_atom.block_id);

        Ok(None)
    }

    fn on_commit(
        &self,
        tx: &mut TStateStore::WriteTransaction<'_>,
        local_committee_info: &CommitteeInfo,
        local_committee: &Committee<TStateStore::Addr>,
        commit_qc: &ProposalCertificate,
        commit_qc_id: &PcId,
        block: &Block,
    ) -> Result<Vec<TransactionPoolRecord>, HotStuffError> {
        let committed_transactions = self.finalize_block(
            tx,
            local_committee_info,
            local_committee,
            commit_qc,
            commit_qc_id,
            block,
        )?;
        debug!(
            target: LOG_TARGET,
            "✅ COMMIT block {}",
            block,
        );
        self.publish_event(HotstuffEvent::BlockCommitted {
            epoch: block.epoch(),
            block_id: *block.id(),
            height: block.height(),
        });
        Ok(committed_transactions)
    }

    fn on_lock_block(
        &self,
        tx: &mut TStateStore::WriteTransaction<'_>,
        new_locked_block: &Block,
    ) -> Result<(), HotStuffError> {
        info!(
            target: LOG_TARGET,
            "🔒️ LOCKED BLOCK: {}",
            new_locked_block,
        );

        // Release all locks for Aborted transactions since these can never be committed after accept
        SubstateRecord::unlock_all(
            tx,
            new_locked_block
                .all_local_accept()
                .filter(|a| a.decision.is_abort())
                .map(|t| &t.id),
        )?;

        // Remove the orphaned chains
        // This also releases any locks for fork blocks.
        new_locked_block.remove_orphaned_blocks(tx)?;
        new_locked_block.lock_executions(tx)?;

        Ok(())
    }

    fn publish_event(&self, event: HotstuffEvent) {
        if let Some(sender) = self.tx_events.upgrade() {
            let _ignore = sender.send(event);
        }
    }

    fn finalize_block(
        &self,
        tx: &mut TStateStore::WriteTransaction<'_>,
        local_committee_info: &CommitteeInfo,
        local_committee: &Committee<TStateStore::Addr>,
        commit_qc: &ProposalCertificate,
        commit_qc_id: &PcId,
        block: &Block,
    ) -> Result<Vec<TransactionPoolRecord>, HotStuffError> {
        if block.is_dummy() {
            block.increment_leader_failure_count(tx, self.config.consensus_constants.liveness_thresholds())?;

            // Nothing to do here for empty dummy blocks. Just mark the block as committed.
            block.commit_block_without_state_changes(tx, commit_qc_id)?;
            return Ok(vec![]);
        }

        // Committing a block applies the change set this node saved when it accepted the block. A block it holds no
        // change set for can only be committed by taking the committee's state through sync.
        if !tx.blocks_is_executed(block.id())? {
            return Err(HotStuffError::NeedsSync {
                reason: format!(
                    "the committee committed block {block}, which this node did not accept, so it holds none of the \
                     block's state changes"
                ),
            });
        }

        info!(target: LOG_TARGET, "🌳 Finalizing block {}", block);

        // This moves the stage update from pending to current for all transactions on the commit block
        self.transaction_pool.confirm_all_transitions(tx, &block.as_leaf())?;

        for atom in block.all_foreign_proposals() {
            // TODO: we need to keep these ATM to send them if a node needs to catch up
            atom.set_status(tx, ForeignProposalStatus::Confirmed, Some(block.id()))?;
        }

        // NOTE: this must happen before we commit the substate diff because the state transitions use this version
        let pending = block.remove_pending_tree_diff_and_return(tx)?;
        let mut state_tree = ShardedStateTree::new(tx);
        let version_updates = state_tree.commit_diffs(pending)?;
        let tx = state_tree.into_transaction();
        // The state version proofs just indexed name this block, which is pruned with its epoch, so they are served
        // with the commit proof stored here. A storage error fails the commit; a block that breaks a commit-proof
        // invariant is logged, and the node keeps committing, as it does when it cannot index the proofs.
        if index_committed_block_state_versions(tx, block, &version_updates)? {
            match generate_block_commit_proof(&**tx, commit_qc, block) {
                Ok(commit_proof) => {
                    tx.block_commit_proofs_insert(block.id(), &CommittedBlockProof::new(commit_proof).to_bytes())?
                },
                Err(err @ HotStuffError::StorageError(_)) => return Err(err),
                Err(err) => error!(
                    target: LOG_TARGET,
                    "BUG: cannot generate the commit proof of {block}, so its state versions cannot be proven to \
                     syncing peers once it is pruned: {err}"
                ),
            }
        }

        {
            let _timer = TraceTimer::debug(LOG_TARGET, "commit_block");
            let downed = block.commit_block(tx, commit_qc_id, &version_updates)?;
            index_substate_down_proofs(
                tx,
                downed,
                self.config.state_history_length,
                committed_block_commit_proof,
            )?;
        }

        let finalized_transactions = {
            let _timer = TraceTimer::debug(LOG_TARGET, "remove finalized transactions");
            self.transaction_pool
                .remove_all(tx, block.all_finalising_transactions_ids())?
        };

        // Whenever we commit a block that will result in an abort for a transaction, we can remove lock conflicts to
        // allow other "blocked" transactions to be proposed.
        TransactionLockConflicts::remove_for_transactions(tx, block.all_aborting_transaction_ids())?;
        TransactionLockConflicts::remove_for_block(tx, block.id())?;

        if !finalized_transactions.is_empty() {
            let _timer =
                TraceTimer::debug(LOG_TARGET, "finalize transactions").with_iterations(finalized_transactions.len());
            TransactionRecord::finalize_all(tx, block.epoch(), &finalized_transactions)?;

            debug!(
                target: LOG_TARGET,
                "✅ {} transactions finalized",
                finalized_transactions.len(),
            );
        }

        let total_transaction_fee = block.calculate_total_transaction_fee();
        if total_transaction_fee > 0 {
            info!(
                target: LOG_TARGET,
                "🪙 Validator fee ({}, Total Fees Paid = {}) for block {}",
                block.total_leader_fee(),
                total_transaction_fee,
                block,
            );
        }

        let signers = justify_signers(block, local_committee_info, local_committee)?;
        let thresholds = self.config.consensus_constants.liveness_thresholds();
        tx.validator_epoch_stats_updates(
            block.justify().epoch(),
            block.height(),
            signers
                .into_iter()
                .map(|pk| ValidatorStatsUpdate::new(pk, thresholds).record_vote()),
        )?;
        block.clear_leader_failure_count(tx, thresholds)?;

        Ok(finalized_transactions)
    }
}

/// The public keys of the members who signed the justify of the committed `block`.
fn justify_signers<'a, TAddr: PartialEq>(
    block: &Block,
    local_committee_info: &CommitteeInfo,
    local_committee: &'a Committee<TAddr>,
) -> Result<Vec<&'a RistrettoPublicKeyBytes>, HotStuffError> {
    // The justify's signer bitmap indexes the committee of its epoch, which must be the local committee.
    let justify = block.justify();
    if justify.epoch() != local_committee_info.epoch() {
        return Err(HotStuffError::InvariantError(format!(
            "Committed block {block} is justified by a certificate from epoch {}, but the local committee is for \
             epoch {}",
            justify.epoch(),
            local_committee_info.epoch()
        )));
    }
    justify
        .signatures()
        .signers()
        .iter()
        .map(|index| {
            local_committee
                .get(index)
                .map(|member| &member.public_key)
                .ok_or_else(|| {
                    HotStuffError::InvariantError(format!(
                        "Justify of committed block {block} names signer {index}, but the local committee has {} \
                         members",
                        local_committee.len()
                    ))
                })
        })
        .collect()
}

/// Consensus rule: a block is rejected (no-vote) when its total transaction execution weight exceeds
/// `max_validation_weight`, EXCEPT when it carries at most one transaction command. The single-command
/// exemption preserves liveness: an individually-heavy transaction must remain committable (and the
/// static weight over-estimates its real execution). This bounds the work a leader can force replicas to
/// do per block. Must be evaluated identically on every node — keep it a pure function of these inputs.
fn exceeds_block_validation_weight(
    block_execution_weight: u64,
    num_transaction_commands: usize,
    max_validation_weight: u64,
) -> bool {
    num_transaction_commands > 1 && block_execution_weight > max_validation_weight
}

/// Ratifies the next epoch's validator set hash that an end-of-epoch block names against this node's own. A block
/// that names none is refused whatever this node knows, and a node that has not assigned the next epoch's committees
/// abstains.
fn check_next_epoch_validator_set(
    local: Option<ValidatorSetHash>,
    proposed: Option<ValidatorSetHash>,
) -> Option<NoVoteReason> {
    let Some(proposed) = proposed else {
        return Some(NoVoteReason::EndOfEpochValidatorSetMissing);
    };
    let Some(local) = local else {
        return Some(NoVoteReason::EndOfEpochValidatorSetNotKnown);
    };
    (local != proposed).then_some(NoVoteReason::EndOfEpochValidatorSetMismatch { local, proposed })
}

/// Consensus rule: the end-of-epoch block is the last block of its epoch to carry commands. The epoch checkpoint and
/// the next epoch's genesis are both built from its state, so the blocks that extend it, up to and including the one
/// that commits it, must leave that state as it is. This holds whether or not the end-of-epoch block has committed: a
/// node whose next-epoch processing is deferred keeps voting in the epoch after the commit.
fn check_no_commands_after_epoch_end<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    parent: &Block,
    block: &Block,
) -> Result<Option<NoVoteReason>, StorageError> {
    if block.commands().is_empty() || !parent.is_epoch_end_proposed_in_chain(tx)? {
        return Ok(None);
    }
    Ok(Some(NoVoteReason::CommandsAfterEndOfEpoch))
}

/// Consensus rule: an end-of-epoch block that changes the number of committees may not leave a transaction in
/// `LocalPrepared` on the chain it extends. Validators move to new shard groups at such a boundary and drop their
/// transaction pools, so that transaction would keep its substate locks with no record left to finalize it. The rule
/// cannot wedge the epoch: a transaction that is ready for LocalAccept can be accepted as usual, and a replica that
/// sees such a next epoch accepts the abort of one still waiting on foreign shard groups (see
/// [`abort_local_prepared_for_epoch_end`]), so a leader can always clear them first.
///
/// The pool is read with the pending updates of the chain ending at `justify_block_id`, the block the end-of-epoch
/// block's certificate justifies, which is the chain a leader checks before proposing it.
fn check_epoch_end_drained<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    justify_block_id: &BlockId,
    next_epoch_changes_layout: bool,
) -> Result<Option<NoVoteReason>, StorageError> {
    if !next_epoch_changes_layout {
        return Ok(None);
    }
    let is_prepared_in_flight =
        tx.transaction_pool_any_in_chain(justify_block_id, |rec| rec.current_stage().is_local_prepared())?;
    Ok(is_prepared_in_flight.then_some(NoVoteReason::EndOfEpochWithTransactionsInLocalPrepared))
}

/// Consensus rule: a `ForeignProposal` command atom must reference a stored proposal that is neither confirmed nor
/// marked invalid, whose committed header names the same shard group as the atom, and whose commit proof carries at
/// least one command entry.
///
/// A proposal is stored as `New` only once `validate_foreign_proposal_contents` accepts it, which refuses an empty
/// commit proof, so every `New` record a proposer selects passes this check.
pub(super) fn check_foreign_proposal_record(
    fp_atom: &ForeignProposalAtom,
    fp: &ForeignProposalRecord,
) -> Option<NoVoteReason> {
    if matches!(fp.status(), ForeignProposalStatus::Confirmed) {
        return Some(NoVoteReason::ForeignProposalAlreadyConfirmed);
    }
    if matches!(fp.status(), ForeignProposalStatus::Invalid) {
        return Some(NoVoteReason::ForeignProposalInvalid {
            block_id: fp_atom.block_id,
        });
    }
    let Some(actual_shard_group) = fp.proposal().shard_group_checked() else {
        return Some(NoVoteReason::ForeignProposalProcessingFailed);
    };
    if actual_shard_group != fp_atom.shard_group {
        return Some(NoVoteReason::ForeignProposalShardGroupMismatch {
            block_id: fp_atom.block_id,
            atom: fp_atom.shard_group,
            actual: actual_shard_group,
        });
    }
    if fp.proposal().commit_proof().commands().is_empty() {
        return Some(NoVoteReason::ForeignProposalProcessingFailed);
    }
    None
}

#[cfg(test)]
mod tests {
    use tari_consensus_types::Decision;
    use tari_ootle_storage::consensus_models::TransactionPoolStage;

    use super::*;

    mod exceeds_block_validation_weight {
        use super::*;

        #[test]
        fn within_budget_is_allowed() {
            assert!(!exceeds_block_validation_weight(10_000, 50, 15_000));
            // Exactly at the cap is allowed.
            assert!(!exceeds_block_validation_weight(15_000, 50, 15_000));
        }

        #[test]
        fn over_budget_with_multiple_commands_is_rejected() {
            assert!(exceeds_block_validation_weight(15_001, 2, 15_000));
            assert!(exceeds_block_validation_weight(31_000, 500, 15_000));
        }

        #[test]
        fn single_heavy_transaction_is_always_allowed() {
            // A single transaction heavier than the whole cap must stay committable (liveness).
            assert!(!exceeds_block_validation_weight(1_000_000, 1, 15_000));
            assert!(!exceeds_block_validation_weight(0, 0, 15_000));
        }
    }

    mod check_ready_to_leave_stage {
        use tari_engine_types::SubstateVersion;
        use tari_ootle_storage::consensus_models::{Evidence, TransactionPoolRecord};
        use tari_ootle_transaction::TransactionId;
        use tari_template_lib_types::ComponentAddress;

        use super::*;
        use crate::hotstuff::command_sequencer::check_ready_to_leave_stage;

        fn input_group() -> ShardGroup {
            ShardGroup::new(0, 127)
        }

        fn local_group() -> ShardGroup {
            ShardGroup::new(128, 255)
        }

        /// A committed record at LocalAccepted for a transaction whose local output shard group has accepted. The input
        /// shard group has prepared, and has also accepted when `input_group_accepted` is set.
        fn local_accepted_record(input_group_accepted: bool) -> TransactionPoolRecord {
            let mut evidence = Evidence::empty();
            let input = evidence
                .add_shard_group(input_group())
                .insert_unpledged_input(ComponentAddress::from_array([1; 32]).into())
                .set_prepare_qc(PcId::zero());
            if input_group_accepted {
                input.set_accept_qc(PcId::zero());
            }
            evidence
                .add_shard_group(local_group())
                .insert_output(ComponentAddress::from_array([2; 32]).into(), SubstateVersion::ZERO)
                .set_accept_qc(PcId::zero());

            TransactionPoolRecord::load(
                TransactionId::new([1; 32]),
                evidence,
                false,
                0,
                None,
                TransactionPoolStage::LocalAccepted,
                None,
                Decision::Commit,
                None,
                None,
                input_group_accepted,
                Epoch(1),
                None,
                time::OffsetDateTime::now_utc(),
                None,
                0,
                0,
                false,
            )
        }

        #[test]
        fn all_accept_before_the_input_shard_group_accepted_is_not_voted_for() {
            let reason = check_ready_to_leave_stage(&local_accepted_record(false), local_group(), "AllAccept");
            assert!(
                matches!(
                    reason,
                    Some(NoVoteReason::ProposedBeforeShardGroupsReady {
                        stage: TransactionPoolStage::LocalAccepted,
                        ..
                    })
                ),
                "unexpected reason: {reason:?}"
            );
        }

        #[test]
        fn all_accept_after_every_shard_group_accepted_is_voted_for() {
            let reason = check_ready_to_leave_stage(&local_accepted_record(true), local_group(), "AllAccept");
            assert!(reason.is_none(), "unexpected reason: {reason:?}");
        }
    }

    mod check_next_epoch_validator_set {
        use super::*;

        fn hash(byte: u8) -> ValidatorSetHash {
            ValidatorSetHash::from_array([byte; 32])
        }

        #[test]
        fn a_matching_set_is_voted_for() {
            assert!(check_next_epoch_validator_set(Some(hash(1)), Some(hash(1))).is_none());
        }

        #[test]
        fn a_different_set_is_refused_naming_both() {
            let reason = check_next_epoch_validator_set(Some(hash(1)), Some(hash(2)));
            assert!(
                matches!(
                    reason,
                    Some(NoVoteReason::EndOfEpochValidatorSetMismatch { local, proposed })
                        if local == hash(1) && proposed == hash(2)
                ),
                "{reason:?}"
            );
        }

        #[test]
        fn an_unknown_local_set_abstains() {
            let reason = check_next_epoch_validator_set(None, Some(hash(1)));
            assert!(
                matches!(reason, Some(NoVoteReason::EndOfEpochValidatorSetNotKnown)),
                "{reason:?}"
            );
        }

        #[test]
        fn a_block_without_a_set_is_refused_whatever_this_node_knows() {
            for local in [Some(hash(1)), None] {
                let reason = check_next_epoch_validator_set(local, None);
                assert!(
                    matches!(reason, Some(NoVoteReason::EndOfEpochValidatorSetMissing)),
                    "{reason:?}"
                );
            }
        }
    }

    mod check_no_commands_after_epoch_end {
        use tari_consensus_types::{BlockId, ProposalCertificate, ShardGroupAccumulatedData};
        use tari_crypto::tari_utilities::epoch_time::EpochTime;
        use tari_ootle_common_types::{ExtraData, NodeHeight, NumPreshards, ProtocolVersion};
        use tari_ootle_storage::consensus_models::{BlockCommands, BlockHeader, EndEpochAtom};
        use tari_ootle_transaction::Network;
        use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
        use tempfile::TempDir;

        use super::*;

        const NUM_PRESHARDS: NumPreshards = NumPreshards::P256;
        const NETWORK: Network = Network::LocalNet;

        struct Chain {
            store: RocksDbStateStore<String>,
            tip: Block,
            epoch: Epoch,
            _tmp: TempDir,
        }

        impl Chain {
            fn new() -> Self {
                let tmp = tempfile::tempdir().unwrap();
                let store = RocksDbStateStore::open(tmp.path(), DatabaseOptions::default()).unwrap();
                let zero = Block::zero_block(NETWORK, NUM_PRESHARDS);
                store
                    .with_write_tx(|tx| {
                        zero.justify().save(tx)?;
                        zero.insert(tx)
                    })
                    .unwrap();
                Self {
                    store,
                    epoch: zero.epoch(),
                    tip: zero,
                    _tmp: tmp,
                }
            }

            /// Blocks pushed from here on are in the next epoch.
            fn next_epoch(&mut self) -> &mut Self {
                self.epoch += Epoch(1);
                self
            }

            fn child(&self, commands: BlockCommands) -> Block {
                let parent = &self.tip;
                let justify = ProposalCertificate::new(
                    *parent.id().hash(),
                    *parent.id(),
                    parent.height(),
                    parent.epoch(),
                    ShardGroup::all_shards(NUM_PRESHARDS),
                    ProposalCertificate::genesis_signatures(),
                    QuorumDecision::Accept,
                );
                let header = BlockHeader::create_unsigned(
                    NETWORK,
                    ProtocolVersion::V0,
                    *parent.id(),
                    justify.calculate_id(),
                    None,
                    parent.height() + NodeHeight(1),
                    self.epoch,
                    ShardGroup::all_shards(NUM_PRESHARDS),
                    RistrettoPublicKeyBytes::default(),
                    FixedHash::zero(),
                    &commands,
                    0,
                    EpochTime::now().as_u64(),
                    FixedHash::zero(),
                    ShardGroupAccumulatedData::default(),
                    ExtraData::new(),
                )
                .unwrap();
                Block::new(header, justify, commands, None)
            }

            /// Appends a block carrying `commands` to the tip, committing it if `commit` is set.
            fn push(&mut self, commands: BlockCommands, commit: bool) -> &mut Self {
                let block = self.child(commands);
                self.store
                    .with_write_tx(|tx| {
                        block.justify().save(tx)?;
                        block.insert(tx)?;
                        if commit {
                            tx.blocks_set_qcs(block.id(), Some(&block.justify().calculate_id()), None)?;
                        }
                        Ok::<_, StorageError>(())
                    })
                    .unwrap();
                self.tip = block;
                self
            }

            /// The rule's decision on a candidate that extends the tip and carries `commands`.
            fn decide(&self, commands: BlockCommands) -> Option<NoVoteReason> {
                let candidate = self.child(commands);
                self.store
                    .with_read_tx(|tx| check_no_commands_after_epoch_end(tx, &self.tip, &candidate))
                    .unwrap()
            }
        }

        fn none() -> BlockCommands {
            BlockCommands::empty()
        }

        fn foreign_proposal() -> BlockCommands {
            BlockCommands::init([Command::ForeignProposal(ForeignProposalAtom {
                block_id: BlockId::new(FixedHash::new([7; 32])),
                shard_group: ShardGroup::new(0, 127),
            })])
            .unwrap()
        }

        fn end_epoch() -> BlockCommands {
            BlockCommands::init([Command::EndEpoch(EndEpochAtom::new(FixedHash::new([9; 32])))]).unwrap()
        }

        fn assert_rejected(reason: Option<NoVoteReason>) {
            assert!(
                matches!(reason, Some(NoVoteReason::CommandsAfterEndOfEpoch)),
                "unexpected reason: {reason:?}"
            );
        }

        #[test]
        fn commands_extending_the_epoch_end_are_not_voted_for() {
            let mut chain = Chain::new();
            chain.push(foreign_proposal(), true).push(end_epoch(), false);
            assert_rejected(chain.decide(foreign_proposal()));
        }

        #[test]
        fn commands_extending_a_pending_epoch_end_are_not_voted_for() {
            let mut chain = Chain::new();
            chain
                .push(foreign_proposal(), true)
                .push(end_epoch(), false)
                .push(none(), false)
                .push(none(), false);
            assert_rejected(chain.decide(foreign_proposal()));
        }

        #[test]
        fn commands_in_the_block_that_commits_the_epoch_end_are_not_voted_for() {
            let mut chain = Chain::new();
            chain
                .push(foreign_proposal(), true)
                .push(end_epoch(), true)
                .push(none(), false)
                .push(none(), false);
            assert_rejected(chain.decide(foreign_proposal()));
        }

        #[test]
        fn commands_after_the_epoch_end_and_its_successors_commit_are_not_voted_for() {
            let mut chain = Chain::new();
            chain
                .push(foreign_proposal(), true)
                .push(end_epoch(), true)
                .push(none(), true)
                .push(none(), true)
                .push(none(), false)
                .push(none(), false);
            assert_rejected(chain.decide(foreign_proposal()));
        }

        #[test]
        fn a_second_epoch_end_is_not_voted_for() {
            let mut chain = Chain::new();
            chain
                .push(foreign_proposal(), true)
                .push(end_epoch(), true)
                .push(none(), false);
            assert_rejected(chain.decide(end_epoch()));
        }

        #[test]
        fn an_empty_block_extending_the_epoch_end_is_voted_for() {
            let mut chain = Chain::new();
            chain
                .push(foreign_proposal(), true)
                .push(end_epoch(), true)
                .push(none(), false);
            let reason = chain.decide(none());
            assert!(reason.is_none(), "unexpected reason: {reason:?}");
        }

        #[test]
        fn commands_without_an_epoch_end_are_voted_for() {
            let mut chain = Chain::new();
            chain
                .push(foreign_proposal(), true)
                .push(none(), true)
                .push(none(), false)
                .push(foreign_proposal(), false);
            let reason = chain.decide(foreign_proposal());
            assert!(reason.is_none(), "unexpected reason: {reason:?}");
        }

        #[test]
        fn commands_after_the_previous_epochs_end_are_voted_for() {
            let mut chain = Chain::new();
            chain
                .push(foreign_proposal(), true)
                .push(end_epoch(), true)
                .push(none(), true)
                .push(none(), true)
                .next_epoch()
                .push(none(), true)
                .push(none(), false);
            let reason = chain.decide(foreign_proposal());
            assert!(reason.is_none(), "unexpected reason: {reason:?}");
        }

        #[test]
        fn the_epoch_end_itself_is_voted_for() {
            let mut chain = Chain::new();
            chain.push(foreign_proposal(), true).push(none(), false);
            let reason = chain.decide(end_epoch());
            assert!(reason.is_none(), "unexpected reason: {reason:?}");
        }

        mod check_epoch_end_drained {
            use tari_ootle_storage::consensus_models::{Evidence, TransactionPoolStatusUpdate};
            use tari_ootle_transaction::TransactionId;

            use super::*;

            impl Chain {
                /// Pools a transaction and moves it to `stage` in a pending update at the tip.
                fn pool_transaction_at(&self, stage: TransactionPoolStage) -> &Self {
                    let id = TransactionId::new([3; 32]);
                    let tip = self.tip.as_leaf();
                    self.store
                        .with_write_tx(|tx| {
                            tx.transaction_pool_insert_new(
                                id,
                                Decision::Commit,
                                &Evidence::empty(),
                                false,
                                false,
                                Epoch(1),
                                0,
                            )?;
                            let mut rec = tx.transaction_pool_get_all(1)?.pop().unwrap();
                            rec.set_next_stage_and_readiness(stage, tip.shard_group()).unwrap();
                            tx.transaction_pool_add_pending_update(&tip, &TransactionPoolStatusUpdate::new(rec, false))
                        })
                        .unwrap();
                    self
                }

                /// The rule's decision on an end-of-epoch block whose certificate justifies the tip.
                fn decide_epoch_end(&self, next_epoch_changes_layout: bool) -> Option<NoVoteReason> {
                    self.store
                        .with_read_tx(|tx| check_epoch_end_drained(tx, self.tip.id(), next_epoch_changes_layout))
                        .unwrap()
                }
            }

            #[test]
            fn an_epoch_end_that_changes_the_committees_with_a_prepared_transaction_is_not_voted_for() {
                let mut chain = Chain::new();
                chain.push(foreign_proposal(), true).push(none(), false);
                chain.pool_transaction_at(TransactionPoolStage::LocalPrepared);
                let reason = chain.decide_epoch_end(true);
                assert!(
                    matches!(reason, Some(NoVoteReason::EndOfEpochWithTransactionsInLocalPrepared)),
                    "unexpected reason: {reason:?}"
                );
            }

            #[test]
            fn an_epoch_end_that_keeps_the_committees_is_voted_for_with_a_prepared_transaction() {
                let mut chain = Chain::new();
                chain.push(foreign_proposal(), true).push(none(), false);
                chain.pool_transaction_at(TransactionPoolStage::LocalPrepared);
                let reason = chain.decide_epoch_end(false);
                assert!(reason.is_none(), "unexpected reason: {reason:?}");
            }

            #[test]
            fn an_epoch_end_that_changes_the_committees_is_voted_for_once_nothing_is_prepared() {
                let mut chain = Chain::new();
                chain.push(foreign_proposal(), true).push(none(), false);
                let reason = chain.decide_epoch_end(true);
                assert!(reason.is_none(), "unexpected reason: {reason:?}");

                chain.pool_transaction_at(TransactionPoolStage::LocalAccepted);
                let reason = chain.decide_epoch_end(true);
                assert!(reason.is_none(), "unexpected reason: {reason:?}");
            }
        }
    }

    mod handle {
        use std::collections::HashMap;

        use tari_consensus_types::{PcId, ProposalCertificate, ShardGroupAccumulatedData};
        use tari_engine_types::{
            creation_check::CreationCheck,
            substate::{Substate, SubstateId},
        };
        use tari_ootle_common_types::{
            ExtraData,
            NodeHeight,
            NumPreshards,
            ProtocolVersion,
            SubstateRequirement,
            SubstateVersion,
            VotePower,
        };
        use tari_ootle_storage::consensus_models::{
            BlockCommands,
            BlockHeaderMetadata,
            BlockPledge,
            CommandsCommitProof,
            Evidence,
            ForeignProposal,
            ForeignProposalRecord,
            ForeignProposalStatus,
            LockedEpoch,
            MultiShardAtom,
            TransactionExecution,
            TransactionPoolStatusUpdate,
        };
        use tari_ootle_transaction::{Network, Transaction, TransactionId};
        use tari_sidechain::{SidechainBlockCommitProof, SidechainBlockHeader};
        use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
        use tari_template_lib_types::{ComponentAddress, crypto::SchnorrSignatureBytes};
        use tempfile::TempDir;

        use super::*;
        use crate::{
            consensus_constants::ConsensusConstants,
            traits::{BlockTransactionExecutorError, UncommittedChainTemplates},
        };

        const NUM_PRESHARDS: NumPreshards = NumPreshards::P256;
        const NETWORK: Network = Network::LocalNet;

        #[derive(Debug, Clone)]
        struct NoopExecutor;

        impl BlockTransactionExecutor<RocksDbStateStore<String>> for NoopExecutor {
            fn execute(
                &self,
                _transaction: &Transaction,
                _locked_epoch: LockedEpoch,
                _input_fanout: u32,
                _resolved_inputs: &HashMap<SubstateRequirement, Substate>,
                _templates: &UncommittedChainTemplates,
                _creation_check: &dyn CreationCheck,
            ) -> Result<TransactionExecution, BlockTransactionExecutorError> {
                unimplemented!("no transaction execution needed in handle tests")
            }
        }

        struct VoterFixture {
            store: RocksDbStateStore<String>,
            voter: OnReadyToVoteOnLocalBlock<RocksDbStateStore<String>, NoopExecutor>,
            committee_info: CommitteeInfo,
            committee: Committee<String>,
            genesis: Block,
            _tmp: TempDir,
        }

        impl VoterFixture {
            fn new() -> Self {
                let tmp = tempfile::tempdir().unwrap();
                let store = RocksDbStateStore::open(tmp.path(), DatabaseOptions::default()).unwrap();
                let shard_group = ShardGroup::all_shards(NUM_PRESHARDS);
                let zero = Block::zero_block(NETWORK, NUM_PRESHARDS);
                let genesis = Block::genesis(
                    NETWORK,
                    ProtocolVersion::V0,
                    Epoch::zero(),
                    FixedHash::zero(),
                    shard_group,
                    FixedHash::zero(),
                    None,
                    Default::default(),
                );
                store
                    .with_write_tx(|tx| {
                        zero.justify().save(tx)?;
                        zero.insert(tx)?;
                        tx.blocks_set_qcs(zero.id(), Some(&PcId::zero()), Some(&PcId::zero()))?;
                        genesis.justify().save(tx)?;
                        genesis.insert(tx)?;
                        let gen_qc_id = genesis.justify().calculate_id();
                        tx.blocks_set_qcs(genesis.id(), Some(&gen_qc_id), Some(&gen_qc_id))?;
                        genesis.as_locked().set(tx)?;
                        genesis.as_leaf().set(tx)?;
                        genesis.justify().as_high_pc().set(tx)?;
                        Ok::<_, HotStuffError>(())
                    })
                    .unwrap();

                let config = HotstuffConfig {
                    network: NETWORK,
                    sidechain_id: None,
                    consensus_constants: ConsensusConstants::devnet(1),
                    state_history_length: 10,
                    pinned_checkpoint_epochs: 2,
                    state_tree_cleanup_interval: std::time::Duration::from_secs(60),
                    epoch_gc_interval: std::time::Duration::from_secs(60),
                    epoch_end_grace_period: std::time::Duration::from_secs(5),
                    epoch_end_drain_period: std::time::Duration::from_secs(5),
                    catch_up_request_timeout: std::time::Duration::from_secs(15),
                };
                let (tx_events, _rx_events) = broadcast::channel(16);
                let manager = ConsensusTransactionManager::new(NoopExecutor, 2160, usize::MAX, usize::MAX);
                let voter = OnReadyToVoteOnLocalBlock::new(
                    RistrettoPublicKey::default(),
                    config,
                    TransactionPool::new(),
                    tx_events.downgrade(),
                    manager,
                );
                let committee_info =
                    CommitteeInfo::new(NUM_PRESHARDS, 1, 1, shard_group, Epoch::zero(), VotePower::of(1));
                let committee = Committee::new(vec![]);
                Self {
                    store,
                    voter,
                    committee_info,
                    committee,
                    genesis,
                    _tmp: tmp,
                }
            }

            fn make_block(
                &self,
                parent: &Block,
                justify_block: &Block,
                height: NodeHeight,
                commands: BlockCommands,
                state_merkle_root: FixedHash,
                timestamp: u64,
            ) -> Block {
                let justify = ProposalCertificate::new(
                    justify_block.header().calculate_hash(),
                    *justify_block.parent(),
                    justify_block.height(),
                    justify_block.epoch(),
                    ShardGroup::all_shards(NUM_PRESHARDS),
                    ProposalCertificate::genesis_signatures(),
                    QuorumDecision::Accept,
                );
                Block::create(
                    NETWORK,
                    ProtocolVersion::V0,
                    *parent.id(),
                    justify,
                    None,
                    height,
                    Epoch::zero(),
                    ShardGroup::all_shards(NUM_PRESHARDS),
                    RistrettoPublicKeyBytes::default(),
                    commands,
                    state_merkle_root,
                    0,
                    SchnorrSignatureBytes::zero(),
                    timestamp,
                    FixedHash::zero(),
                    ShardGroupAccumulatedData::default(),
                    ExtraData::new(),
                )
                .unwrap()
            }

            fn handle_block(&mut self, block: &Block) -> BlockDecision {
                self.store
                    .with_write_tx(|tx| {
                        block.justify().save(tx)?;
                        if !tx.blocks_exists(block.id())? {
                            block.insert(tx)?;
                        }
                        let valid_block = ValidBlock::new(block.clone());
                        let mut change_set = ProposedBlockChangeSet::new(block.as_leaf());
                        self.voter.handle(
                            tx,
                            &valid_block,
                            &self.committee_info,
                            &self.committee,
                            &RistrettoPublicKeyBytes::default(),
                            false,
                            LocalNextEpochView::default(),
                            false,
                            ProposalOrigin::Live,
                            &mut change_set,
                        )
                    })
                    .unwrap()
            }
        }

        #[test]
        #[expect(clippy::too_many_lines)]
        fn rejected_block_does_not_mark_justified_block_processed_so_next_valid_block_applies_evidence() {
            let mut fixture = VoterFixture::new();
            let shard_group = fixture.committee_info.shard_group();
            let tx_id = TransactionId::new([7; 32]);
            let mut evidence = Evidence::empty();
            let substate_id: SubstateId = ComponentAddress::from_array([1; 32]).into();
            evidence
                .add_shard_group(shard_group)
                .insert_output(substate_id, SubstateVersion::ZERO);

            let prepare_atom = MultiShardAtom {
                id: tx_id,
                decision: Decision::Commit,
                evidence: evidence.clone(),
                transaction_fee: 100,
                leader_fee: None,
                fee_intent_only: false,
                locked_epoch: fixture.genesis.to_locked_epoch(),
            };
            let block_1 = fixture.make_block(
                &fixture.genesis,
                &fixture.genesis,
                NodeHeight(1),
                BlockCommands::init([Command::LocalPrepare(prepare_atom)]).unwrap(),
                FixedHash::zero(),
                10,
            );

            // Insert `block_1` and its pending transaction pool update at `LocalPrepared` (not yet justified).
            fixture
                .store
                .with_write_tx(|tx| {
                    block_1.justify().save(tx)?;
                    block_1.insert(tx)?;
                    tx.transaction_pool_insert_new(tx_id, Decision::Commit, &evidence, false, false, Epoch(10), 0)?;
                    let mut rec = tx.transaction_pool_get_all(1)?.pop().unwrap();
                    rec.set_next_stage_and_readiness(TransactionPoolStage::LocalPrepared, shard_group)
                        .unwrap();
                    rec.set_ready(false);
                    tx.transaction_pool_add_pending_update(
                        &block_1.as_leaf(),
                        &TransactionPoolStatusUpdate::new(rec, false),
                    )?;
                    Ok::<_, HotStuffError>(())
                })
                .unwrap();

            // `bad_block_2` justifies `block_1` but fails `decide_what_to_vote` (`StateMerkleRootMismatch`).
            let bad_block_2 = fixture.make_block(
                &block_1,
                &block_1,
                NodeHeight(2),
                BlockCommands::empty(),
                FixedHash::from([0xBB; 32]),
                20,
            );
            let bad_decision = fixture.handle_block(&bad_block_2);
            assert_eq!(bad_decision.local_decision, None);
            assert!(matches!(
                bad_decision.no_vote_reason,
                Some(NoVoteReason::StateMerkleRootMismatch)
            ));

            // Because `bad_block_2` was rejected and its change_set was discarded, `block_1` must NOT be marked
            // as `has_justify_qc() == true` in storage.
            let block_1_has_qc = fixture
                .store
                .with_read_tx(|tx| Ok::<_, HotStuffError>(Block::get(tx, block_1.id())?.has_justify_qc()))
                .unwrap();
            assert!(
                !block_1_has_qc,
                "rejected block must not mark justified_block as processed"
            );

            // Now view 2 is filled by a dummy/empty block `block_2` extending `block_1`, and `good_block_3` at
            // height 3 justifies `block_2`. `process_newly_justified_block` walks from `block_2` back to `block_1`
            // (because `block_1.has_justify_qc()` is still false) and records `prepare_qc` for `tx_id`.
            let block_2 = fixture.make_block(
                &block_1,
                &fixture.genesis,
                NodeHeight(2),
                BlockCommands::empty(),
                FixedHash::zero(),
                21,
            );
            fixture
                .store
                .with_write_tx(|tx| {
                    block_2.justify().save(tx)?;
                    block_2.insert(tx)
                })
                .unwrap();

            let good_block_3 = fixture.make_block(
                &block_2,
                &block_2,
                NodeHeight(3),
                BlockCommands::empty(),
                FixedHash::zero(),
                30,
            );
            let good_decision = fixture.handle_block(&good_block_3);
            assert_eq!(good_decision.local_decision, Some(QuorumDecision::Accept));

            let (block_1_has_qc, rec) = fixture
                .store
                .with_read_tx(|tx| {
                    let b1 = Block::get(tx, block_1.id())?;
                    let chain = tx.pending_chain_get(good_block_3.id())?;
                    let rec = tx.transaction_pool_get_for_blocks_in_chain(&chain, &tx_id)?;
                    Ok::<_, HotStuffError>((b1.has_justify_qc(), rec))
                })
                .unwrap();
            assert!(block_1_has_qc);
            assert_eq!(
                rec.evidence().get(&shard_group).and_then(|e| e.prepare_qc()),
                Some(&good_block_3.justify().calculate_id())
            );
            assert!(rec.is_ready());
        }

        fn foreign_proposal_record() -> ForeignProposalRecord {
            let metadata = BlockHeaderMetadata {
                total_leader_fee: 0,
                timestamp: 0,
                extra_data: Default::default(),
                timeout_certificate_id: None,
                is_dummy: false,
            };
            let header = SidechainBlockHeader {
                network: NETWORK.as_byte(),
                protocol_version: 0,
                parent_id: Default::default(),
                justify_id: Default::default(),
                height: 1,
                epoch: 0,
                epoch_hash: Default::default(),
                shard_group: tari_sidechain::ShardGroup {
                    start: 0,
                    end_inclusive: 127,
                },
                proposed_by: Default::default(),
                state_merkle_root: Default::default(),
                command_merkle_root: Default::default(),
                transaction_merkle_root: Default::default(),
                signature: Default::default(),
                accumulated_data: Default::default(),
                metadata_hash: metadata.calculate_hash(),
            };
            let proof = CommandsCommitProof::new_latest(vec![], SidechainBlockCommitProof {
                header,
                proof_elements: vec![],
            });
            ForeignProposalRecord::new(ForeignProposal::new(proof, BlockPledge::default(), metadata))
        }

        /// The epoch GC removes a confirmed foreign proposal's proposed-in index entry by the block the record names,
        /// so a committed block must name itself on the foreign proposals it confirms, including one that a competing
        /// fork proposed after it.
        #[test]
        fn a_committed_block_confirms_its_foreign_proposals_as_proposed_in_itself() {
            let fixture = VoterFixture::new();
            let foreign_proposal = foreign_proposal_record();
            let commands = || BlockCommands::init([Command::ForeignProposal(foreign_proposal.to_atom())]).unwrap();
            let committed = fixture.make_block(
                &fixture.genesis,
                &fixture.genesis,
                NodeHeight(1),
                commands(),
                FixedHash::zero(),
                10,
            );
            let fork = fixture.make_block(
                &fixture.genesis,
                &fixture.genesis,
                NodeHeight(1),
                commands(),
                FixedHash::zero(),
                11,
            );

            let record = fixture
                .store
                .with_write_tx(|tx| {
                    for block in [&committed, &fork] {
                        block.justify().save(tx)?;
                        block.insert(tx)?;
                    }
                    foreign_proposal.save(tx)?;
                    // The committed block was accepted, so its change set was saved.
                    tx.blocks_set_executed(committed.id())?;
                    for block in [&committed, &fork] {
                        tx.foreign_proposals_set_status(
                            foreign_proposal.block_id(),
                            ForeignProposalStatus::Proposed,
                            Some(block.id()),
                        )?;
                    }
                    let commit_qc = committed.justify();
                    fixture.voter.finalize_block(
                        tx,
                        &fixture.committee_info,
                        &fixture.committee,
                        commit_qc,
                        &commit_qc.calculate_id(),
                        &committed,
                    )?;
                    let [record] = tx
                        .foreign_proposals_get_any([foreign_proposal.block_id()])?
                        .try_into()
                        .unwrap();
                    Ok::<_, HotStuffError>(record)
                })
                .unwrap();

            assert_eq!(record.status(), ForeignProposalStatus::Confirmed);
            assert_eq!(record.proposed_in_block(), Some(committed.id()));
        }

        #[test]
        fn a_foreign_proposal_command_for_an_invalid_foreign_proposal_is_rejected() {
            let mut fixture = VoterFixture::new();
            let foreign_proposal = foreign_proposal_record();
            fixture
                .store
                .with_write_tx(|tx| {
                    foreign_proposal.save(tx)?;
                    tx.foreign_proposals_set_status(foreign_proposal.block_id(), ForeignProposalStatus::Invalid, None)
                })
                .unwrap();

            let commands = BlockCommands::init([Command::ForeignProposal(foreign_proposal.to_atom())]).unwrap();
            let block = fixture.make_block(
                &fixture.genesis,
                &fixture.genesis,
                NodeHeight(1),
                commands,
                FixedHash::zero(),
                10,
            );
            let decision = fixture.handle_block(&block);
            assert!(decision.local_decision.is_none());
            assert!(
                matches!(
                    decision.no_vote_reason,
                    Some(NoVoteReason::ForeignProposalInvalid { block_id }) if block_id == *foreign_proposal.block_id()
                ),
                "unexpected no_vote_reason: {:?}",
                decision.no_vote_reason
            );
        }

        #[test]
        fn a_foreign_proposal_command_with_mismatched_shard_group_is_rejected() {
            let mut fixture = VoterFixture::new();
            let foreign_proposal = foreign_proposal_record();
            fixture.store.with_write_tx(|tx| foreign_proposal.save(tx)).unwrap();

            let forged_shard_group = ShardGroup::new(128, 255);
            assert_ne!(forged_shard_group, foreign_proposal.shard_group_unchecked());
            let mut atom = foreign_proposal.to_atom();
            atom.shard_group = forged_shard_group;
            let commands = BlockCommands::init([Command::ForeignProposal(atom)]).unwrap();
            let block = fixture.make_block(
                &fixture.genesis,
                &fixture.genesis,
                NodeHeight(1),
                commands,
                FixedHash::zero(),
                10,
            );
            let decision = fixture.handle_block(&block);
            assert!(decision.local_decision.is_none());
            assert!(
                matches!(
                    decision.no_vote_reason,
                    Some(NoVoteReason::ForeignProposalShardGroupMismatch { block_id, atom, actual })
                        if block_id == *foreign_proposal.block_id()
                            && atom == forged_shard_group
                            && actual == foreign_proposal.shard_group_unchecked()
                ),
                "unexpected no_vote_reason: {:?}",
                decision.no_vote_reason
            );
        }
    }

    mod check_foreign_proposal_record {
        use tari_common_types::types::CompressedPublicKey;
        use tari_ootle_storage::consensus_models::{
            BlockHeaderMetadata,
            BlockPledge,
            CommandOrHash,
            CommandsCommitProof,
            ForeignProposal,
            ForeignProposalAtom,
            ForeignProposalRecord,
            ForeignProposalStatus,
            PrunedCommand,
        };
        use tari_sidechain::{FixedHash, SidechainBlockCommitProof, SidechainBlockHeader};

        use super::*;

        fn make_record(
            shard_group: ShardGroup,
            commands: Vec<CommandOrHash>,
            status: ForeignProposalStatus,
        ) -> ForeignProposalRecord {
            let commit_proof = CommandsCommitProof::new_latest(commands, SidechainBlockCommitProof {
                header: SidechainBlockHeader {
                    network: 0,
                    protocol_version: 0,
                    parent_id: Default::default(),
                    justify_id: Default::default(),
                    height: 1,
                    epoch: 1,
                    epoch_hash: Default::default(),
                    shard_group: tari_sidechain::ShardGroup {
                        start: shard_group.start().as_u32(),
                        end_inclusive: shard_group.end().as_u32(),
                    },
                    proposed_by: CompressedPublicKey::default(),
                    state_merkle_root: Default::default(),
                    command_merkle_root: Default::default(),
                    transaction_merkle_root: Default::default(),
                    signature: Default::default(),
                    accumulated_data: Default::default(),
                    metadata_hash: Default::default(),
                },
                proof_elements: vec![],
            });
            let proposal = ForeignProposal::new(commit_proof, BlockPledge::default(), BlockHeaderMetadata {
                total_leader_fee: 0,
                timestamp: 0,
                extra_data: Default::default(),
                timeout_certificate_id: None,
                is_dummy: false,
            });
            let block_id = proposal.calculate_block_id();
            ForeignProposalRecord::load(block_id, proposal, None, status)
        }

        #[test]
        fn rejects_invalid_or_confirmed_or_mismatched_shard_group_or_empty_commands() {
            let sg_a = ShardGroup::new(0, 63);
            let sg_b = ShardGroup::new(64, 127);
            let non_empty_cmds = vec![CommandOrHash::Pruned(PrunedCommand {
                hash: FixedHash::from([1u8; 32]),
                full_command_shard_groups: vec![],
            })];

            let valid_rec = make_record(sg_a, non_empty_cmds.clone(), ForeignProposalStatus::New);
            let atom = ForeignProposalAtom {
                block_id: *valid_rec.block_id(),
                shard_group: sg_a,
            };
            assert!(check_foreign_proposal_record(&atom, &valid_rec).is_none());

            let invalid_rec = make_record(sg_a, non_empty_cmds.clone(), ForeignProposalStatus::Invalid);
            assert!(matches!(
                check_foreign_proposal_record(&atom, &invalid_rec),
                Some(NoVoteReason::ForeignProposalInvalid { .. })
            ));

            let confirmed_rec = make_record(sg_a, non_empty_cmds.clone(), ForeignProposalStatus::Confirmed);
            assert!(matches!(
                check_foreign_proposal_record(&atom, &confirmed_rec),
                Some(NoVoteReason::ForeignProposalAlreadyConfirmed)
            ));

            let wrong_sg_atom = ForeignProposalAtom {
                block_id: *valid_rec.block_id(),
                shard_group: sg_b,
            };
            assert!(matches!(
                check_foreign_proposal_record(&wrong_sg_atom, &valid_rec),
                Some(NoVoteReason::ForeignProposalShardGroupMismatch { .. })
            ));

            let empty_cmds_rec = make_record(sg_a, vec![], ForeignProposalStatus::New);
            assert!(matches!(
                check_foreign_proposal_record(&atom, &empty_cmds_rec),
                Some(NoVoteReason::ForeignProposalProcessingFailed)
            ));
        }
    }
}
