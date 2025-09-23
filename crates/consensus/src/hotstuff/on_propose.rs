//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeSet, HashMap},
    fmt::Display,
    num::NonZeroU64,
};

use log::*;
use tari_common_types::types::FixedHash;
use tari_consensus_types::{Decision, HighPc, HighestSeenBlock, LeafBlock, ProposalCertificate, TimeoutCertificate};
use tari_crypto::tari_utilities::epoch_time::EpochTime;
use tari_engine_types::{commit_result::RejectReason, ToByteType};
use tari_epoch_manager::EpochManagerReader;
use tari_ootle_common_types::{
    committee::CommitteeInfo,
    displayable::Displayable,
    optional::Optional,
    Epoch,
    ExtraData,
    NodeHeight,
};
use tari_ootle_storage::{
    consensus_models::{
        calculate_leader_fee,
        Block,
        BlockHeader,
        BlockTransactionExecution,
        BookkeepingModel,
        Command,
        EvictNodeAtom,
        ForeignProposal,
        ForeignProposalRecord,
        PendingShardStateTreeDiff,
        TransactionAtom,
        TransactionExecution,
        TransactionPool,
        TransactionPoolRecord,
        TransactionPoolStage,
        TransactionRecord,
        ValidatorConsensusStats,
    },
    StateStore,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tari_transaction::TransactionId;
use tokio::task;

use crate::{
    hotstuff::{
        apply_leader_fee_to_substate_store,
        block_change_set::ProposedBlockChangeSet,
        calculate_state_merkle_root,
        epoch_state::EpochState,
        error::HotStuffError,
        filter_diff_for_committee,
        foreign_proposal_processor::process_foreign_block,
        process_newly_justified_block,
        substate_store::PendingSubstateStore,
        transaction_manager::{
            ConsensusTransactionManager,
            EvidenceOrExecution,
            LocalPreparedTransaction,
            PledgedTransaction,
            PreparedTransaction,
            TransactionLockConflicts,
        },
        HotstuffConfig,
    },
    messages::{HotstuffMessage, ProposalMessage},
    tracing::TraceTimer,
    traits::{CertificateStore, ConsensusSpec, OutboundMessaging, ValidatorSignerService, WriteableSubstateStore},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_propose";

struct NextBlock {
    block: Block,
    foreign_proposals: Vec<ForeignProposal>,
    executed_transactions: HashMap<TransactionId, TransactionExecution>,
    lock_conflicts: TransactionLockConflicts,
}

#[derive(Debug, Clone)]
pub struct OnPropose<TConsensusSpec: ConsensusSpec> {
    config: HotstuffConfig,
    store: TConsensusSpec::StateStore,
    epoch_manager: TConsensusSpec::EpochManager,
    transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
    transaction_manager: ConsensusTransactionManager<TConsensusSpec::TransactionExecutor, TConsensusSpec::StateStore>,
    signing_service: TConsensusSpec::SignerService,
    outbound_messaging: TConsensusSpec::OutboundMessaging,
}

impl<TConsensusSpec> OnPropose<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        config: HotstuffConfig,
        store: TConsensusSpec::StateStore,
        epoch_manager: TConsensusSpec::EpochManager,
        transaction_pool: TransactionPool<TConsensusSpec::StateStore>,
        transaction_manager: ConsensusTransactionManager<
            TConsensusSpec::TransactionExecutor,
            TConsensusSpec::StateStore,
        >,
        signing_service: TConsensusSpec::SignerService,
        outbound_messaging: TConsensusSpec::OutboundMessaging,
    ) -> Self {
        Self {
            config,
            store,
            epoch_manager,
            transaction_pool,
            transaction_manager,
            signing_service,
            outbound_messaging,
        }
    }

    #[allow(clippy::too_many_lines)]
    pub async fn handle(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        next_height: NodeHeight,
        local_claim_public_key: RistrettoPublicKeyBytes,
        highest_seen_block: HighestSeenBlock,
        dummy_block: Option<LeafBlock>,
        propose_high_tc: Option<TimeoutCertificate>,
        propose_epoch_end: bool,
    ) -> Result<(), HotStuffError> {
        let epoch = epoch_state.epoch();
        let local_committee_info = *epoch_state.local_committee_info();
        let local_committee = epoch_state.local_committee();
        let epoch_hash = *epoch_state.epoch_hash();
        let _timer = TraceTimer::info(LOG_TARGET, "OnPropose");

        let on_propose = self.clone();

        let (next_block, foreign_proposals) = task::spawn_blocking(move || {
            on_propose.store.with_write_tx(|tx| {
                let high_qc = HighPc::get(&**tx, epoch)?;
                let high_qc_cert = ProposalCertificate::get(&**tx, epoch, high_qc.id())?;
                let next_block = on_propose.build_next_block(
                    tx,
                    epoch,
                    next_height,
                    highest_seen_block,
                    dummy_block,
                    high_qc_cert,
                    &local_committee_info,
                    &local_claim_public_key,
                    epoch_hash,
                    propose_high_tc,
                    propose_epoch_end,
                )?;

                let NextBlock {
                    block: next_block,
                    foreign_proposals,
                    executed_transactions,
                    lock_conflicts,
                } = next_block;

                lock_conflicts.save_for_block(tx, next_block.id())?;

                // Add executions for this block
                if !executed_transactions.is_empty() {
                    debug!(
                        target: LOG_TARGET,
                        "Saving {} executed transaction(s) for block {}",
                        executed_transactions.len(),
                        next_block.id()
                    );
                }
                for (transaction_id, executed) in executed_transactions {
                    executed
                        .for_block(next_block.as_leaf(), transaction_id)
                        .insert_if_required(tx)?;
                }

                next_block.as_last_proposed().set(tx)?;

                Ok::<_, HotStuffError>((next_block, foreign_proposals))
            })
        })
        .await??;

        info!(
            target: LOG_TARGET,
            "🌿 [{}] PROPOSING new block {} to {} validators. justifies: {}, parent: {}",
            self.signing_service.public_key(),
            next_block,
            local_committee.len(),
            next_block.justify().height(),
            next_block.parent()
        );

        self.broadcast_local_proposal(next_block, foreign_proposals, &local_committee_info)
            .await?;

        Ok(())
    }

    pub async fn broadcast_local_proposal(
        &mut self,
        next_block: Block,
        foreign_proposals: Vec<ForeignProposal>,
        local_committee_info: &CommitteeInfo,
    ) -> Result<(), HotStuffError> {
        let epoch = next_block.epoch();
        let leaf_block = next_block.as_leaf();
        let msg = HotstuffMessage::new_proposal(ProposalMessage {
            block: next_block,
            foreign_proposals,
        });

        // Broadcast to local and foreign committees
        self.outbound_messaging.send_self(msg.clone()).await?;

        // If we are the only VN in this committee, no need to multicast
        if local_committee_info.num_shard_group_members() <= 1 {
            info!(
                target: LOG_TARGET,
                "🌿 This node is the only member of the local committee. No need to multicast proposal {leaf_block}",
            );
        } else {
            let committee = self
                .epoch_manager
                .get_committee_by_shard_group(epoch, local_committee_info.shard_group(), None)
                .await?;

            info!(
                target: LOG_TARGET,
                "🌿 Broadcasting local proposal to {}/{} local committee members {}",
                committee.len(), local_committee_info.num_shard_group_members(), leaf_block,
            );

            if let Err(err) = self.outbound_messaging.multicast(committee.into_addresses(), msg).await {
                warn!(
                    target: LOG_TARGET,
                    "Failed to multicast proposal to local committee: {}",
                    err
                );
            }
        }

        Ok(())
    }

    /// Returns Ok(None) if the command cannot be sequenced yet due to lock conflicts.
    fn transaction_pool_record_to_command(
        &self,
        start_of_chain_id: &LeafBlock,
        pool_tx: TransactionPoolRecord,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TConsensusSpec::StateStore>,
        executed_transactions: &mut HashMap<TransactionId, TransactionExecution>,
        lock_conflicts: &mut TransactionLockConflicts,
    ) -> Result<Option<Command>, HotStuffError> {
        match pool_tx.current_stage() {
            TransactionPoolStage::New => self.prepare_transaction(
                start_of_chain_id,
                pool_tx,
                local_committee_info,
                substate_store,
                executed_transactions,
                lock_conflicts,
            ),
            // Leader thinks all foreign PREPARE pledges have been received (condition for LocalPrepared stage to be
            // ready)
            TransactionPoolStage::LocalPrepared => self.local_accept_transaction(
                start_of_chain_id,
                local_committee_info,
                pool_tx,
                substate_store,
                executed_transactions,
            ),

            // Leader thinks that all foreign ACCEPT pledges have been received and, we are ready to accept the result
            // (COMMIT/ABORT)
            TransactionPoolStage::LocalAccepted => {
                self.accept_transaction(start_of_chain_id, &pool_tx, local_committee_info, substate_store)
            },
            // Not reachable as there is nothing to propose for these stages. To confirm that all local nodes
            // agreed with the Accept, more (possibly empty) blocks with QCs will be
            // proposed and accepted, otherwise the Accept block will not be committed.
            TransactionPoolStage::AllAccepted |
            TransactionPoolStage::SomeAccepted |
            TransactionPoolStage::LocalOnly => {
                unreachable!(
                    "It is invalid for TransactionPoolStage::{} to be ready to propose",
                    pool_tx.current_stage()
                )
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    fn build_next_block(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        epoch: Epoch,
        next_height: NodeHeight,
        highest_seen_block: HighestSeenBlock,
        dummy_block: Option<LeafBlock>,
        high_qc_certificate: ProposalCertificate,
        local_committee_info: &CommitteeInfo,
        local_claim_public_key_bytes: &RistrettoPublicKeyBytes,
        epoch_hash: FixedHash,
        propose_high_tc: Option<TimeoutCertificate>,
        can_propose_epoch_end: bool,
    ) -> Result<NextBlock, HotStuffError> {
        let high_qc_id = high_qc_certificate.calculate_id();
        let justify_block = Block::get(tx, &high_qc_certificate.calculate_block_id())?;
        let start_of_chain_block = highest_seen_block;
        let parent_block = dummy_block.unwrap_or_else(|| highest_seen_block.as_leaf());

        let should_not_propose_commands = can_propose_epoch_end || {
            // TODO: prevent proposers from proposing transactions after an epoch end command is in the justified
            // pending chain, regardless of whether we see the end of epoch or not (race condition).
            // If the last justified/parent block is an epoch end block, we dont propose commands since the block will
            // be rejected
            let block = Block::get(tx, highest_seen_block.block_id())?;
            block.is_epoch_end()
        };

        let mut total_leader_fee = 0;

        let batch = if should_not_propose_commands {
            ProposalBatch::default()
        } else {
            self.fetch_next_proposal_batch(tx, local_committee_info, start_of_chain_block)?
        };

        let mut substate_store = PendingSubstateStore::new(
            tx,
            start_of_chain_block.as_leaf(),
            self.config.consensus_constants.num_preshards,
        );

        debug!(target: LOG_TARGET, "🌿 PROPOSE: {batch}");
        let mut executed_transactions = HashMap::new();
        let mut commands = if can_propose_epoch_end {
            BTreeSet::from_iter([Command::EndEpoch])
        } else {
            BTreeSet::from_iter(
                batch
                    .foreign_proposals
                    .iter()
                    .map(|fp| Command::ForeignProposal(fp.to_atom()))
                    .chain(
                        batch
                            .evict_nodes
                            .into_iter()
                            .map(|public_key| Command::EvictNode(EvictNodeAtom { public_key })),
                    ),
            )
        };

        // NOTE: the block for the change set is not used.
        let mut change_set = ProposedBlockChangeSet::new(start_of_chain_block.as_leaf());

        // No need to include evidence from justified block if no transactions are included in the next block
        if !batch.transactions.is_empty() {
            // TODO(protocol-efficiency): We should process any foreign proposals included in this block to include
            // evidence. And that should determine if they are ready. However this is difficult because we
            // get the batch from the database which isnt aware of which foreign proposals we're going to
            // propose. This is why the system currently never proposes foreign proposals affecting a
            // transaction in the same block for LocalPrepare/LocalAccept.
            for fp in &batch.foreign_proposals {
                if let Err(err) = process_foreign_block(
                    tx,
                    &high_qc_certificate.as_leaf_block(),
                    fp,
                    local_committee_info,
                    &mut substate_store,
                    &mut change_set,
                ) {
                    warn!(
                        target: LOG_TARGET,
                        "Failed to process foreign proposal: {}. Not proposing...",
                        err
                    );
                    // TODO: should mark as invalid?
                    continue;
                }
            }

            // Add all (ABORT) executions that may have resulted from foreign proposals
            executed_transactions.extend(change_set.take_all_transaction_executions());

            if !justify_block.has_justify_qc() {
                // TODO: we dont need to process transactions here that are not in the batch
                process_newly_justified_block::<TConsensusSpec::StateStore>(
                    tx,
                    &justify_block,
                    high_qc_id,
                    local_committee_info,
                    &mut change_set,
                )?;
            }
        }

        // batch is empty for is_empty, is_epoch_end and is_epoch_start blocks
        let timer = TraceTimer::info(LOG_TARGET, "Generating commands").with_iterations(batch.transactions.len());
        let mut lock_conflicts = TransactionLockConflicts::new();
        for mut transaction in batch.transactions {
            // Apply the transaction updates (if any) that occurred as a result of the justified block.
            // This allows us to propose evidence in the next block that relates to transactions in the justified block.
            change_set.apply_transaction_update(&mut transaction);
            if let Some(command) = self.transaction_pool_record_to_command(
                &start_of_chain_block.as_leaf(),
                transaction,
                local_committee_info,
                &mut substate_store,
                &mut executed_transactions,
                &mut lock_conflicts,
            )? {
                total_leader_fee += command
                    .committing()
                    .and_then(|tx| tx.leader_fee.as_ref())
                    .map(|f| f.fee)
                    .unwrap_or(0);
                // TODO: a BTreeSet changes the order from the original batch. Uncertain if this is a problem since the
                // proposer also processes transactions in the completed block order, however on_propose does perform
                // some operations (e.g. prepare, execute) in batch order. To ensure correctness, we should process
                // on_propose in canonical order.
                commands.insert(command);
            }
        }
        timer.done();

        debug!(
            target: LOG_TARGET,
            "command(s) for next block: [{}]",
            commands.display()
        );

        // Add proposer fee substate
        if total_leader_fee > 0 {
            // Apply leader fee to substate store before we calculate the state root
            apply_leader_fee_to_substate_store(
                &mut substate_store,
                local_claim_public_key_bytes,
                local_committee_info.shard_group().start(),
                local_committee_info.num_preshards(),
                total_leader_fee,
            )?;
        }

        let timer = TraceTimer::info(LOG_TARGET, "Propose calculate state root");
        let pending_tree_diffs =
            PendingShardStateTreeDiff::get_all_up_to_commit_block(tx, start_of_chain_block.block_id())?;
        let (state_root, _) = calculate_state_merkle_root(
            tx,
            local_committee_info.shard_group(),
            pending_tree_diffs,
            substate_store.changes()
                .iter()
                // Calculate for local shards only and the global shard
                .filter(|ch| local_committee_info.shard_group().contains_or_global(&ch.shard())),
        )?;
        timer.done();

        let mut header = BlockHeader::create_unsigned(
            self.config.network,
            *parent_block.block_id(),
            high_qc_id,
            next_height,
            epoch,
            local_committee_info.shard_group(),
            self.signing_service.public_key().to_byte_type(),
            state_root,
            &commands,
            total_leader_fee,
            EpochTime::now().as_u64(),
            epoch_hash,
            ExtraData::new(),
        )?;

        let signature = self.signing_service.sign(&header);
        header.set_signature(signature.to_byte_type());

        let next_block = Block::new(header, high_qc_certificate, commands, propose_high_tc);

        Ok(NextBlock {
            block: next_block,
            foreign_proposals: batch.foreign_proposals,
            executed_transactions,
            lock_conflicts,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn fetch_next_proposal_batch(
        &self,
        tx: &<<TConsensusSpec as ConsensusSpec>::StateStore as StateStore>::ReadTransaction<'_>,
        local_committee_info: &CommitteeInfo,
        start_of_chain_block: HighestSeenBlock,
    ) -> Result<ProposalBatch, HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "fetch_next_proposal_batch");
        // Foreign proposals are weighted by 10x. Each foreign proposal can contain max_number_commands_in_block*inputs
        // substate pledges. TODO: this is a loosey goosey upper bound. We need to evaluate this, find an acceptable
        // upper bound or perhaps re-design how foreign proposals work
        const FP_WEIGHT_MULTIPLIER: usize = 10;
        let foreign_proposals = ForeignProposalRecord::get_all_new(
            tx,
            start_of_chain_block.block_id(),
            self.config.consensus_constants.max_number_commands_in_block / FP_WEIGHT_MULTIPLIER,
        )?;

        if !foreign_proposals.is_empty() {
            debug!(
                target: LOG_TARGET,
                "🌿 Found {} foreign proposals for next block",
                foreign_proposals.len()
            );
        }

        let mut remaining_block_size = subtract_block_size_checked(
            Some(self.config.consensus_constants.max_number_commands_in_block),
            foreign_proposals.len() * FP_WEIGHT_MULTIPLIER,
        );

        let evict_nodes = remaining_block_size
            .map(|max| {
                let num_evicted =
                    ValidatorConsensusStats::count_number_evicted_nodes(tx, start_of_chain_block.epoch())?;
                // TODO: technically, we should not evict more than 1/3 of the voting power, not the number of nodes
                // (but this is currently the same thing)
                let max_allowed_to_evict = u64::from(local_committee_info.max_failure_shard_group_members())
                    .saturating_sub(num_evicted)
                    .min(max as u64);
                ValidatorConsensusStats::get_nodes_to_evict(
                    tx,
                    start_of_chain_block.block_id(),
                    self.config.consensus_constants.missed_proposal_evict_threshold,
                    max_allowed_to_evict,
                )
            })
            .transpose()?
            .unwrap_or_default();

        if !evict_nodes.is_empty() {
            debug!(
                target: LOG_TARGET,
                "🌿 Found {} EVICT nodes for next block",
                evict_nodes.len()
            )
        }

        remaining_block_size = subtract_block_size_checked(remaining_block_size, evict_nodes.len());

        let transactions = remaining_block_size
            .map(|size| {
                self.transaction_pool
                    .get_batch_for_next_block(tx, size, start_of_chain_block.block_id())
            })
            .transpose()?
            .unwrap_or_default();

        Ok(ProposalBatch {
            foreign_proposals: foreign_proposals.into_iter().map(|fp| fp.into_proposal()).collect(),
            transactions,
            evict_nodes,
            commands: vec![],
        })
    }

    #[allow(clippy::too_many_lines)]
    fn prepare_transaction(
        &self,
        parent_block: &LeafBlock,
        mut pool_tx: TransactionPoolRecord,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TConsensusSpec::StateStore>,
        executed_transactions: &mut HashMap<TransactionId, TransactionExecution>,
        lock_conflicts: &mut TransactionLockConflicts,
    ) -> Result<Option<Command>, HotStuffError> {
        info!(
            target: LOG_TARGET,
            "👨‍🔧 PROPOSE: PREPARE transaction {}",
            pool_tx.transaction_id(),
        );

        let prepared = self
            .transaction_manager
            .prepare(substate_store, local_committee_info, &pool_tx, *parent_block)
            .map_err(|e| HotStuffError::TransactionExecutorError(e.to_string()))?;

        if prepared.lock_status().is_any_failed() && !prepared.lock_status().is_hard_conflict() {
            warn!(
                target: LOG_TARGET,
                "⚠️ Transaction {} has lock conflicts, but no hard conflicts. Skipping proposing this transaction...",
                pool_tx.transaction_id(),
            );

            lock_conflicts.add(
                *pool_tx.transaction_id(),
                prepared.into_lock_status().into_lock_conflicts(),
            );
            return Ok(None);
        }

        match prepared {
            PreparedTransaction::LocalOnly(local) => match *local {
                LocalPreparedTransaction::Accept { execution, .. } => {
                    pool_tx
                        .set_local_decision(execution.decision())
                        .set_transaction_fee(execution.transaction_fee())
                        .set_evidence(execution.to_evidence(
                            local_committee_info.num_preshards(),
                            local_committee_info.num_committees(),
                        ));

                    info!(
                        target: LOG_TARGET,
                        "🏠️ Transaction {} is local only, proposing LocalOnly",
                        pool_tx.transaction_id(),
                    );

                    if pool_tx.current_decision().is_commit() {
                        let involved = NonZeroU64::new(1).expect("1 > 0");
                        let leader_fee = calculate_leader_fee(
                            pool_tx.transaction_fee(),
                            involved,
                            self.config.consensus_constants.fee_exhaust_divisor,
                        );
                        pool_tx.set_leader_fee(leader_fee);
                        let diff = execution.result().finalize.result.any_accept().ok_or_else(|| {
                            HotStuffError::InvariantError(format!(
                                "prepare_transaction: Transaction {} has COMMIT decision but execution failed when \
                                 proposing",
                                pool_tx.transaction_id(),
                            ))
                        })?;

                        if let Err(err) = substate_store.put_diff(diff) {
                            error!(
                                target: LOG_TARGET,
                                "🔒 Failed to write to temporary state store for transaction {} for LocalOnly: {}. Skipping proposing this transaction...",
                                pool_tx.transaction_id(),
                                err,
                            );
                            // Only error if it is not related to lock errors
                            let _err = err.ok_lock_failed()?;
                            return Ok(None);
                        }
                    }

                    executed_transactions.insert(*pool_tx.transaction_id(), execution);

                    let atom = pool_tx.get_current_transaction_atom();
                    Ok(Some(Command::LocalOnly(atom)))
                },
                LocalPreparedTransaction::EarlyAbort { execution } => {
                    info!(
                        target: LOG_TARGET,
                        "⚠️ Transaction is LOCAL-ONLY EARLY ABORT, proposing LocalOnly({}, ABORT)",
                        pool_tx.transaction_id()
                    );
                    pool_tx
                        .set_local_decision(execution.decision())
                        .set_transaction_fee(execution.transaction_fee())
                        .no_leader_fee()
                        .merge_evidence(execution.to_evidence(
                            local_committee_info.num_preshards(),
                            local_committee_info.num_committees(),
                        ));

                    executed_transactions.insert(*pool_tx.transaction_id(), execution);
                    let atom = pool_tx.get_current_transaction_atom();
                    Ok(Some(Command::LocalOnly(atom)))
                },
            },

            PreparedTransaction::MultiShard(multishard) => {
                match multishard.into_evidence_or_execution() {
                    EvidenceOrExecution::Execution { execution } => {
                        // CASE: All inputs are local and outputs are foreign (i.e. the transaction is
                        // executed), or all inputs are foreign and this shard
                        // group is output only and, we've already received all pledges.
                        pool_tx.update_from_execution(
                            local_committee_info.num_preshards(),
                            local_committee_info.num_committees(),
                            &execution,
                        );

                        // TODO: this is kinda hacky - we may not be involved in the transaction after ABORT execution,
                        // but this would be invalid so we ensure that we are added to evidence. Ideally, we wouldn't
                        // sequence this transaction at all - investigate.
                        pool_tx
                            .evidence_mut()
                            .add_shard_group(local_committee_info.shard_group());

                        if execution.decision().is_commit() {
                            let involves_inputs = pool_tx.evidence().has_inputs(local_committee_info.shard_group());
                            if !involves_inputs {
                                let num_involved_shard_groups = pool_tx.evidence().num_shard_groups();
                                let involved = NonZeroU64::new(num_involved_shard_groups as u64).ok_or_else(|| {
                                    HotStuffError::InvariantError("Number of involved shard groups is 0".to_string())
                                })?;
                                let leader_fee = calculate_leader_fee(
                                    pool_tx.transaction_fee(),
                                    involved,
                                    self.config.consensus_constants.fee_exhaust_divisor,
                                );
                                pool_tx.set_leader_fee(leader_fee);
                            }
                        }

                        executed_transactions.insert(*pool_tx.transaction_id(), *execution);
                    },
                    EvidenceOrExecution::Evidence { evidence, .. } => {
                        // CASE: All local inputs were resolved. We need to continue with consensus to get the
                        // foreign inputs/outputs.
                        pool_tx.set_local_decision(Decision::Commit)
                            // Set partial evidence using local inputs and known outputs.
                            // NOTE: we could have evidence for initial sequence from foreign proposals, so we must not overwrite it
                            .merge_evidence(evidence);
                    },
                }

                info!(
                    target: LOG_TARGET,
                    "🌍 Transaction involves foreign shard groups, proposing Prepare({}, {})",
                    pool_tx.transaction_id(),
                    pool_tx.current_decision(),
                );

                if pool_tx.current_decision().is_abort() {
                    let atom = pool_tx.get_current_transaction_atom();
                    return Ok(Some(Command::LocalAccept(atom)));
                }
                if pool_tx
                    .evidence()
                    .is_committee_output_only(local_committee_info.shard_group())
                {
                    // No prepare phase needed for output-only transactions. All foreign shards have prepared inputs
                    // and the output shard groups need to execute and accept outputs.
                    debug!(
                        target: LOG_TARGET,
                        "ℹ️ Transaction {} is output-only for {}, proposing LocalAccept",
                        pool_tx.transaction_id(),
                        local_committee_info.shard_group()
                    );
                    let atom = pool_tx.get_current_transaction_atom();
                    Ok(Some(Command::LocalAccept(atom)))
                } else {
                    let atom = pool_tx.get_current_transaction_atom();
                    Ok(Some(Command::LocalPrepare(atom)))
                }
            },
        }
    }

    fn local_accept_transaction(
        &self,
        parent_block: &LeafBlock,
        local_committee_info: &CommitteeInfo,
        mut tx_rec: TransactionPoolRecord,
        substate_store: &mut PendingSubstateStore<TConsensusSpec::StateStore>,
        executed_transactions: &mut HashMap<TransactionId, TransactionExecution>,
    ) -> Result<Option<Command>, HotStuffError> {
        // Only set to abort if either the local or one or more foreign shards decided to ABORT
        if tx_rec.current_decision().is_abort() {
            return Ok(Some(Command::LocalAccept(tx_rec.get_current_transaction_atom())));
        }

        let tx = substate_store.read_transaction();
        let transaction = TransactionRecord::get(tx, tx_rec.transaction_id())?;
        let execution = self.execute_transaction(tx, parent_block, transaction)?;

        // Try to lock all local outputs
        let local_outputs = execution
            .resulting_outputs()
            .iter()
            .filter(|o| local_committee_info.includes_substate_id(o.substate_id()));
        let lock_status = substate_store.try_lock_all(*tx_rec.transaction_id(), local_outputs, false)?;
        if let Some(err) = lock_status.failures().first() {
            warn!(
                target: LOG_TARGET,
                "⚠️ Failed to lock outputs for transaction {}: {}",
                tx_rec.transaction_id(),
                err,
            );
            // If the transaction does not lock, we propose to abort it
            let execution = TransactionExecution::abort(
                tx_rec.transaction_id(),
                RejectReason::FailedToLockOutputs(err.to_string()),
            );
            tx_rec.update_from_execution(
                local_committee_info.num_preshards(),
                local_committee_info.num_committees(),
                &execution,
            );

            executed_transactions.insert(*tx_rec.transaction_id(), execution);
            return Ok(Some(Command::LocalAccept(tx_rec.get_current_transaction_atom())));
        }

        tx_rec.update_from_execution(
            local_committee_info.num_preshards(),
            local_committee_info.num_committees(),
            &execution,
        );
        executed_transactions.insert(*tx_rec.transaction_id(), execution);
        // If we locally decided to ABORT, we are still saying that we think all prepared and, after execution decide to
        // ABORT. When we enter the acceptance phase, we will propose SomeAccept for this case.
        let atom = self.get_transaction_atom_with_leader_fee(&tx_rec)?;
        Ok(Some(Command::LocalAccept(atom)))
    }

    fn accept_transaction(
        &self,
        parent_block: &LeafBlock,
        tx_rec: &TransactionPoolRecord,
        local_committee_info: &CommitteeInfo,
        substate_store: &mut PendingSubstateStore<TConsensusSpec::StateStore>,
    ) -> Result<Option<Command>, HotStuffError> {
        if tx_rec.current_decision().is_abort() {
            return Ok(Some(Command::SomeAccept(tx_rec.get_current_transaction_atom())));
        }

        let tx = substate_store.read_transaction();
        let execution = tx_rec
            .get_pending_execution_for_block(tx, parent_block)
            .optional()?
            .ok_or_else(|| {
                HotStuffError::InvariantError(format!(
                    "accept_transaction: Transaction {} has COMMIT decision but execution is missing",
                    tx_rec.transaction_id(),
                ))
            })?;
        let diff = execution.result().finalize.any_accept().ok_or_else(|| {
            HotStuffError::InvariantError(format!(
                "local_accept_transaction: Transaction {} has COMMIT decision but execution failed when proposing",
                tx_rec.transaction_id(),
            ))
        })?;
        let filtered_diff = filter_diff_for_committee(local_committee_info, diff);
        if let Err(err) = substate_store.put_diff(&filtered_diff) {
            error!(
                target: LOG_TARGET,
                "🔒 Failed to write to temporary state store for transaction {} for Accept: {}. Skipping proposing this transaction...",
                tx_rec.transaction_id(),
                err,
            );
            // Only error if it is not related to lock errors
            let _err = err.ok_lock_failed()?;
            return Ok(None);
        }
        let atom = self.get_transaction_atom_with_leader_fee(tx_rec)?;
        Ok(Some(Command::AllAccept(atom)))
    }

    fn get_transaction_atom_with_leader_fee(
        &self,
        tx_rec: &TransactionPoolRecord,
    ) -> Result<TransactionAtom, HotStuffError> {
        let mut atom = tx_rec.get_current_transaction_atom();
        if tx_rec.current_decision().is_commit() {
            let num_involved_shard_groups = tx_rec.evidence().num_shard_groups();
            let involved = NonZeroU64::new(num_involved_shard_groups as u64).ok_or_else(|| {
                HotStuffError::InvariantError(format!(
                    "PROPOSE: Transaction {} involves zero shard groups",
                    tx_rec.transaction_id(),
                ))
            })?;
            let leader_fee = tx_rec.calculate_leader_fee(involved, self.config.consensus_constants.fee_exhaust_divisor);
            atom.leader_fee = Some(leader_fee);
        }
        Ok(atom)
    }

    fn execute_transaction(
        &self,
        tx: &<TConsensusSpec::StateStore as StateStore>::ReadTransaction<'_>,
        parent_block: &LeafBlock,
        transaction: TransactionRecord,
    ) -> Result<TransactionExecution, HotStuffError> {
        // Should have been executed already if all inputs are local
        if let Some(execution) =
            BlockTransactionExecution::get_pending_for_block(tx, transaction.id(), parent_block).optional()?
        {
            info!(
                target: LOG_TARGET,
                "👨‍🔧 PROPOSE: Using existing transaction execution {} ({})",
                transaction.id(), execution.decision(),
            );
            return Ok(execution.into_transaction_execution());
        }

        let pledged = PledgedTransaction::load_pledges(tx, transaction)?;

        info!(
            target: LOG_TARGET,
            "👨‍🔧 PROPOSE: Executing transaction {} (pledges: {} local, {} foreign)",
            pledged.id(), pledged.local_pledges.len(), pledged.foreign_pledges.len(),
        );

        let executed = self
            .transaction_manager
            .execute(parent_block.epoch(), pledged)
            .map_err(|e| HotStuffError::TransactionExecutorError(e.to_string()))?;

        Ok(executed)
    }
}

#[derive(Default)]
struct ProposalBatch {
    pub foreign_proposals: Vec<ForeignProposal>,
    pub transactions: Vec<TransactionPoolRecord>,
    pub evict_nodes: Vec<RistrettoPublicKeyBytes>,
    pub commands: Vec<Command>,
}

impl Display for ProposalBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} transaction(s), {} foreign proposal(s), {} evict, {} command(s)",
            self.transactions.len(),
            self.foreign_proposals.len(),
            self.evict_nodes.len(),
            self.commands.len()
        )
    }
}

fn subtract_block_size_checked(remaining_block_size: Option<usize>, by: usize) -> Option<usize> {
    remaining_block_size
        .and_then(|sz| sz.checked_sub(by))
        .filter(|sz| *sz > 0)
}
