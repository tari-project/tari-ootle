//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Sequences transactions into a local block. The proposer and every replica run each transaction through the same
//! [`CommandSequencer`] at the same point of the block, against the same change set and substate store. The proposer
//! leaves out a transaction the sequencer refuses. A replica withholds its vote from a block that carries a refused
//! transaction, or a command other than the one the sequencer derives. A sequencing rule written here binds both.

use std::{collections::HashMap, num::NonZeroU64};

use indexmap::IndexMap;
use log::*;
use tari_consensus_types::LeafBlock;
use tari_engine_types::commit_result::RejectReason;
use tari_ootle_common_types::{
    Epoch,
    ShardGroup,
    VersionedSubstateId,
    committee::CommitteeInfo,
    optional::Optional,
    shard::Shard,
};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    consensus_models::{
        Block,
        BlockTransactionExecution,
        Command,
        InvalidEvidenceReason,
        LockConflict,
        LockedEpoch,
        NoVoteReason,
        PendingShardStateTreeDiff,
        TransactionExecution,
        TransactionPoolRecord,
        TransactionPoolStage,
        TransactionRecord,
    },
};
use tari_ootle_transaction::Network;
use tari_sidechain::FixedHash;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    hotstuff::{
        abort_if_local_prepared_expired,
        abort_local_prepared_for_epoch_end,
        apply_leader_fee_to_substate_store,
        block_change_set::ProposedBlockChangeSet,
        calculate_state_merkle_root,
        command_derivation::{
            DerivedCommand,
            derive_local_only_command,
            derive_new_multishard_command,
            pin_new_transaction,
        },
        error::HotStuffError,
        filter_diff_for_committee,
        local_pledged_inputs,
        substate_store::{InputPledge, LockStatus, PendingSubstateStore},
        transaction_manager::{
            ConsensusTransactionManager,
            MultishardLockScope,
            PledgedTransaction,
            PreparedTransaction,
            lock_multishard_execution,
        },
    },
    traits::{BlockTransactionExecutor, WriteableSubstateStore},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::command_sequencer";

/// Why a transaction cannot be sequenced at this point of the block. A proposer leaves the transaction out of the
/// block, and a replica does not vote for a block that sequences it.
#[derive(Debug)]
pub(super) struct Refusal {
    pub reason: NoVoteReason,
    /// The locks that stopped the transaction. A proposer records them so that the transaction is not reconsidered
    /// until they are released.
    pub lock_conflicts: Vec<LockConflict>,
}

impl From<NoVoteReason> for Refusal {
    fn from(reason: NoVoteReason) -> Self {
        Self {
            reason,
            lock_conflicts: Vec::new(),
        }
    }
}

/// The command derived for a transaction, or why it cannot be sequenced.
pub(super) type Sequencing = Result<DerivedCommand, Refusal>;

/// What the commands of a block add to its header totals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct BlockTotals {
    pub leader_fee: u64,
    /// The exhaust burn accumulated by the chain up to and including the block.
    pub exhaust_burn: u128,
}

impl BlockTotals {
    /// Totals for a block that extends a block whose chain has accumulated `exhaust_burn`.
    pub fn extending(exhaust_burn: u128) -> Self {
        Self {
            leader_fee: 0,
            exhaust_burn,
        }
    }

    fn add(&mut self, derived: &DerivedCommand) -> Result<(), HotStuffError> {
        self.leader_fee = self
            .leader_fee
            .checked_add(derived.leader_fee)
            .ok_or_else(|| HotStuffError::InvariantError("Leader fee overflow when summing for block".to_string()))?;
        self.exhaust_burn += u128::from(derived.exhaust_burn);
        Ok(())
    }
}

/// Derives the command for each transaction of a local block and applies what including it changes.
pub(super) struct CommandSequencer<'a, TExecutor, TStateStore> {
    transaction_manager: &'a ConsensusTransactionManager<TExecutor, TStateStore>,
    local_committee_info: &'a CommitteeInfo,
    /// The block whose chain pending executions are read at.
    chain_tip: LeafBlock,
    /// The epoch of the block being sequenced.
    epoch: Epoch,
    /// The pin the block gives the New transactions it sequences.
    block_locked_epoch: LockedEpoch,
    /// The most bytes a proposer lets one shard take in the block. A replica checks the block's writes against the
    /// looser validation limit once every command is evaluated, so it has none here.
    max_proposal_shard_output_bytes: Option<usize>,
}

impl<'a, TExecutor, TStateStore> CommandSequencer<'a, TExecutor, TStateStore>
where
    TStateStore: StateStore,
    TExecutor: BlockTransactionExecutor<TStateStore>,
{
    /// A sequencer for a block this node proposes in `epoch` that extends `chain_tip`.
    pub fn for_proposal(
        transaction_manager: &'a ConsensusTransactionManager<TExecutor, TStateStore>,
        local_committee_info: &'a CommitteeInfo,
        chain_tip: LeafBlock,
        epoch: Epoch,
        block_locked_epoch: LockedEpoch,
        max_shard_output_bytes: usize,
    ) -> Self {
        Self {
            transaction_manager,
            local_committee_info,
            chain_tip,
            epoch,
            block_locked_epoch,
            max_proposal_shard_output_bytes: Some(max_shard_output_bytes),
        }
    }

    /// A sequencer that re-derives the commands of `block`, proposed by another member of the local committee.
    pub fn for_block(
        transaction_manager: &'a ConsensusTransactionManager<TExecutor, TStateStore>,
        local_committee_info: &'a CommitteeInfo,
        block: &Block,
    ) -> Self {
        Self {
            transaction_manager,
            local_committee_info,
            chain_tip: block.as_leaf(),
            epoch: block.epoch(),
            block_locked_epoch: block.to_locked_epoch(),
            max_proposal_shard_output_bytes: None,
        }
    }

    fn is_proposing(&self) -> bool {
        self.max_proposal_shard_output_bytes.is_some()
    }

    fn shard_group(&self) -> ShardGroup {
        self.local_committee_info.shard_group()
    }

    /// Derives the command that moves `pool_record` out of its current stage. `abort_waiting_for_epoch_end` permits
    /// aborting a LocalPrepared transaction that is still waiting on foreign shard groups (see
    /// [`abort_local_prepared_for_epoch_end`]).
    pub fn sequence<TTx: StateStoreReadTransaction>(
        &self,
        pool_record: TransactionPoolRecord,
        abort_waiting_for_epoch_end: bool,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &ProposedBlockChangeSet,
    ) -> Result<Sequencing, HotStuffError> {
        match pool_record.current_stage() {
            TransactionPoolStage::New => self.sequence_new(pool_record, "New", substate_store, change_set),
            TransactionPoolStage::LocalPrepared => {
                self.sequence_local_accept(pool_record, abort_waiting_for_epoch_end, substate_store, change_set)
            },
            TransactionPoolStage::LocalAccepted => self.sequence_accept(pool_record, substate_store, change_set),
            stage => Err(HotStuffError::InvariantError(format!(
                "Transaction {} at stage {stage} has no command to sequence",
                pool_record.id(),
            ))),
        }
    }

    /// Derives the LocalOnly, LocalPrepare or LocalAccept command for a transaction at the New stage. `label` names
    /// the command in the reason given for a transaction that is not ready.
    #[allow(clippy::too_many_lines)]
    pub fn sequence_new<TTx: StateStoreReadTransaction>(
        &self,
        mut pool_record: TransactionPoolRecord,
        label: &'static str,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &ProposedBlockChangeSet,
    ) -> Result<Sequencing, HotStuffError> {
        let transaction_id = *pool_record.id();
        // A New transaction is ready only once it holds every foreign input pledge that prepare needs
        if let Some(reason) = self.check_ready_to_leave_stage(&pool_record, label) {
            return Ok(Err(reason.into()));
        }

        if let Err(pinned_after) = pin_new_transaction(&mut pool_record, &self.block_locked_epoch, self.shard_group()) {
            return Ok(Err(pinned_after.into_no_vote(transaction_id).into()));
        }

        // A foreign proposal in this block may already have aborted the transaction
        let foreign_abort = change_set
            .transaction_execution(&transaction_id)
            .filter(|execution| execution.decision().is_abort())
            .map(|execution| execution.transaction_execution().clone());
        let prepared = match foreign_abort {
            Some(execution) => {
                info!(
                    target: LOG_TARGET,
                    "👨‍🔧 Transaction {transaction_id} in {} is already ABORTED by a foreign proposal",
                    self.chain_tip,
                );
                Ok(PreparedTransaction::new_multishard_executed(
                    execution,
                    LockStatus::new(),
                ))
            },
            None if self.is_proposing() => self.transaction_manager.prepare_for_proposal(
                substate_store,
                self.local_committee_info,
                &pool_record,
                self.chain_tip,
                change_set,
            ),
            None => self.transaction_manager.prepare(
                substate_store,
                self.local_committee_info,
                &pool_record,
                self.chain_tip,
                change_set,
            ),
        }
        .map_err(|e| HotStuffError::TransactionExecutorError(e.to_string()))?;

        if prepared.lock_status().is_deferrable_conflict() {
            let details = prepared
                .lock_status()
                .failures()
                .first()
                .map(ToString::to_string)
                .unwrap_or_default();
            return Ok(Err(Refusal {
                reason: NoVoteReason::ConflictingTransactionProposed {
                    transaction_id,
                    details,
                },
                lock_conflicts: prepared.into_lock_status().into_lock_conflicts(),
            }));
        }

        match prepared {
            PreparedTransaction::PledgeBudgetReached { total_bytes, max_bytes } => Ok(Err(
                NoVoteReason::BlockPledgeBytesExceeded { total_bytes, max_bytes }.into(),
            )),
            PreparedTransaction::InputLoadBudgetReached { total_bytes, max_bytes } => {
                Ok(Err(NoVoteReason::BlockInputLoadBytesExceeded {
                    total_bytes,
                    max_bytes,
                }
                .into()))
            },
            PreparedTransaction::InputsLocked { .. } => Err(HotStuffError::InvariantError(format!(
                "Transaction {transaction_id} was prepared as having its inputs locked without a deferrable lock \
                 conflict",
            ))),
            PreparedTransaction::LocalOnly(local) => {
                let derived = match derive_local_only_command(pool_record, *local, self.local_committee_info) {
                    Ok(derived) => derived,
                    Err(refusal) => return Ok(Err(NoVoteReason::from(refusal).into())),
                };
                if let Some(diff) = derived.committed_diff() {
                    if let Some(refusal) = self.check_proposal_shard_output_bytes(substate_store, diff) {
                        return Ok(Err(refusal));
                    }
                    if let Err(err) = substate_store.put_diff(diff) {
                        // A lock failure or DOWN input is an expected conflict under contention. Any other store
                        // error is fatal.
                        let lock_err = err.ok_lock_failed()?;
                        return Ok(Err(NoVoteReason::ConflictingTransactionProposed {
                            transaction_id,
                            details: lock_err.to_string(),
                        }
                        .into()));
                    }
                }
                Ok(Ok(derived))
            },
            PreparedTransaction::MultiShard(multishard) => Ok(Ok(derive_new_multishard_command(
                pool_record,
                multishard,
                self.local_committee_info,
            )?)),
        }
    }

    /// Derives the LocalAccept command for a transaction at the LocalPrepared stage.
    #[allow(clippy::too_many_lines)]
    pub fn sequence_local_accept<TTx: StateStoreReadTransaction>(
        &self,
        mut pool_record: TransactionPoolRecord,
        abort_waiting_for_epoch_end: bool,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &ProposedBlockChangeSet,
    ) -> Result<Sequencing, HotStuffError> {
        let transaction_id = *pool_record.id();
        let shard_group = self.shard_group();
        let forced_abort = abort_if_local_prepared_expired(&mut pool_record, self.epoch).or_else(|| {
            let is_waiting = abort_waiting_for_epoch_end && !pool_record.is_ready_for_pending_stage(shard_group);
            is_waiting
                .then(|| abort_local_prepared_for_epoch_end(&mut pool_record))
                .flatten()
        });

        // Readiness is evaluated before execution, which may change the decision and with it the readiness condition
        if let Some(reason) = self.check_ready_to_leave_stage(&pool_record, "LocalAccept") {
            return Ok(Err(reason.into()));
        }

        if pool_record.current_decision().is_abort() {
            // Either this or a foreign shard group decided to ABORT. There is nothing to execute.
            return Ok(Ok(DerivedCommand {
                command: Command::LocalAccept(pool_record.get_current_multi_shard_atom()?),
                pool_record,
                execution: forced_abort,
                leader_fee: 0,
                exhaust_burn: 0,
            }));
        }

        let tx = substate_store.read_transaction();
        let transaction = pool_record.get_transaction(tx)?;
        if !transaction.has_all_required_input_pledges(
            tx,
            &self.chain_tip,
            self.local_committee_info,
            change_set.get_foreign_pledges(&transaction_id),
        )? {
            return Ok(Err(NoVoteReason::NotAllForeignInputPledges.into()));
        }

        let locked_epoch = pool_record.locked_epoch().cloned().ok_or_else(|| {
            HotStuffError::InvariantError(format!(
                "Transaction {transaction_id} is at the LocalPrepared stage but has no locked epoch"
            ))
        })?;
        // A foreign proposal in this block may already have replaced the transaction's execution with its fee-intent
        // commit
        let execution = match change_set.transaction_execution(&transaction_id) {
            Some(execution) => execution.transaction_execution().clone(),
            None => self.execute_transaction(substate_store, locked_epoch, transaction, change_set)?,
        };

        let num_preshards = self.local_committee_info.num_preshards();
        let num_committees = self.local_committee_info.num_committees();
        let execution = if execution.decision().is_commit() {
            let pledged_inputs = local_pledged_inputs(&pool_record, shard_group)
                .map(|id| id.to_owned())
                .collect::<Vec<_>>();
            let locked = lock_multishard_execution(
                substate_store,
                self.local_committee_info,
                transaction_id,
                execution,
                MultishardLockScope::LocalOutputs,
                pool_record.is_fee_intent_only(),
            )
            .map_err(|e| HotStuffError::TransactionExecutorError(e.to_string()))?;
            if let Some(err) = locked.lock_status.failures().first() {
                info!(
                    target: LOG_TARGET,
                    "⚠️ Failed to lock outputs for transaction {transaction_id} in {}: {err}. Accepting it as ABORT",
                    self.chain_tip,
                );
                let execution =
                    TransactionExecution::abort(&transaction_id, RejectReason::FailedToLockOutputs(err.to_string()));
                pool_record
                    .update_from_execution(num_preshards, num_committees, &execution)
                    .no_leader_fee();
                execution
            } else {
                // Preparing the transaction bounded what its inputs alone may pledge, so only the block budget applies
                let pledge = substate_store.pledge_inputs(
                    pledged_inputs.iter().map(VersionedSubstateId::as_versioned_ref),
                    usize::MAX,
                )?;
                if let InputPledge::BlockBudgetReached { total_bytes, max_bytes } = pledge {
                    return Ok(Err(
                        NoVoteReason::BlockPledgeBytesExceeded { total_bytes, max_bytes }.into()
                    ));
                }
                pool_record.update_from_execution(num_preshards, num_committees, &locked.execution);
                if locked.fee_intent_only {
                    pool_record.set_fee_intent_only(true);
                }
                locked.execution
            }
        } else {
            pool_record.update_from_execution(num_preshards, num_committees, &execution);
            execution
        };

        if pool_record.current_decision().is_commit() {
            // The leader fee is fixed here and paid by the AllAccept that finalizes the transaction
            let involved = NonZeroU64::new(pool_record.evidence().num_shard_groups() as u64).ok_or_else(|| {
                HotStuffError::InvariantError(format!("Transaction {transaction_id} involves zero shard groups"))
            })?;
            let leader_fee = pool_record.calculate_leader_fee(involved);
            pool_record.set_leader_fee(leader_fee);
        }

        Ok(Ok(DerivedCommand {
            command: Command::LocalAccept(pool_record.get_current_multi_shard_atom()?),
            pool_record,
            execution: Some(execution),
            leader_fee: 0,
            exhaust_burn: 0,
        }))
    }

    /// Derives the AllAccept or SomeAccept command for a transaction at the LocalAccepted stage.
    pub fn sequence_accept<TTx: StateStoreReadTransaction>(
        &self,
        pool_record: TransactionPoolRecord,
        substate_store: &mut PendingSubstateStore<TTx>,
        change_set: &ProposedBlockChangeSet,
    ) -> Result<Sequencing, HotStuffError> {
        let transaction_id = *pool_record.id();
        let is_commit = pool_record.current_decision().is_commit();
        let label = if is_commit { "AllAccept" } else { "SomeAccept" };
        if let Some(reason) = self.check_ready_to_leave_stage(&pool_record, label) {
            return Ok(Err(reason.into()));
        }

        if !is_commit {
            return Ok(Ok(DerivedCommand {
                command: Command::SomeAccept(pool_record.get_current_multi_shard_atom()?),
                pool_record,
                execution: None,
                leader_fee: 0,
                exhaust_burn: 0,
            }));
        }

        let tx = substate_store.read_transaction();
        if !pool_record.has_all_required_foreign_pledges(
            tx,
            self.local_committee_info,
            change_set.get_foreign_pledges(&transaction_id),
        )? {
            return Ok(Err(NoVoteReason::NotAllForeignInputPledges.into()));
        }

        let leader_fee = pool_record.leader_fee().cloned().ok_or_else(|| {
            HotStuffError::InvariantError(format!(
                "Transaction {transaction_id} has COMMIT decision and is at the LocalAccepted stage but has no leader \
                 fee",
            ))
        })?;

        // A foreign proposal earlier in this block may have replaced the stored execution with its fee-intent commit
        let stored_execution;
        let execution = match change_set.transaction_execution(&transaction_id) {
            Some(execution) => execution.transaction_execution(),
            None => {
                stored_execution =
                    BlockTransactionExecution::get_pending_for_block(tx, &transaction_id, &self.chain_tip)
                        .optional()?
                        .ok_or_else(|| {
                            HotStuffError::InvariantError(format!(
                                "Transaction {transaction_id} has COMMIT decision but its execution is missing",
                            ))
                        })?;
                stored_execution.transaction_execution()
            },
        };

        if execution.is_fee_intent_commit != pool_record.is_fee_intent_only() {
            return Ok(Err(NoVoteReason::FeeIntentExecutionMismatch {
                transaction_id,
                fee_intent_only: pool_record.is_fee_intent_only(),
                execution_fee_intent_only: execution.is_fee_intent_commit,
            }
            .into()));
        }

        let diff = execution.result().finalize.any_accept().ok_or_else(|| {
            HotStuffError::InvariantError(format!(
                "Transaction {transaction_id} has COMMIT decision but its execution was rejected",
            ))
        })?;

        // Computed from the local record's evidence: its key order is locally maintained (sorted), whereas an atom's
        // wire-decoded key order is not consensus-checked (evidence equality is order-independent).
        let shard_group = self.shard_group();
        let Some(exhaust_burn) = pool_record
            .evidence()
            .exhaust_burn_portion(leader_fee.exhaust_burn(), shard_group)
        else {
            return Ok(Err(NoVoteReason::InvalidEvidence {
                reason: InvalidEvidenceReason::MissingInvolvedShardGroup { shard_group },
            }
            .into()));
        };

        let diff = filter_diff_for_committee(self.local_committee_info, diff);
        if let Some(refusal) = self.check_proposal_shard_output_bytes(substate_store, &diff) {
            return Ok(Err(refusal));
        }
        if let Err(err) = substate_store.put_diff(&diff) {
            // A lock failure or DOWN input is an expected conflict under contention. Any other store error is fatal.
            let lock_err = err.ok_lock_failed()?;
            return Ok(Err(NoVoteReason::ConflictingTransactionProposed {
                transaction_id,
                details: lock_err.to_string(),
            }
            .into()));
        }

        Ok(Ok(DerivedCommand {
            command: Command::AllAccept(pool_record.get_current_multi_shard_atom()?),
            pool_record,
            execution: None,
            leader_fee: leader_fee.fee(),
            exhaust_burn,
        }))
    }

    /// Records in `change_set` what including `derived` in the block changes, adds it to `totals` and returns its
    /// command.
    pub fn commit(
        &self,
        derived: DerivedCommand,
        change_set: &mut ProposedBlockChangeSet,
        totals: &mut BlockTotals,
    ) -> Result<Command, HotStuffError> {
        totals.add(&derived)?;
        let DerivedCommand {
            command,
            mut pool_record,
            execution,
            ..
        } = derived;
        let transaction_id = *pool_record.id();
        if let Some(execution) = execution {
            if execution.decision().is_abort() {
                change_set.add_abort_execution(transaction_id, execution);
            } else {
                // The derivation may have started from an execution recorded earlier in the block, which this one
                // supersedes
                change_set.take_transaction_execution(&transaction_id);
                change_set.add_transaction_execution(transaction_id, execution)?;
            }
        }
        pool_record.set_next_stage_and_readiness(next_stage(&command)?, self.shard_group())?;
        change_set.set_next_transaction_update(pool_record)?;
        Ok(command)
    }

    fn check_ready_to_leave_stage(
        &self,
        pool_record: &TransactionPoolRecord,
        command: &'static str,
    ) -> Option<NoVoteReason> {
        check_ready_to_leave_stage(pool_record, self.shard_group(), command)
    }

    fn check_proposal_shard_output_bytes<TTx: StateStoreReadTransaction>(
        &self,
        substate_store: &PendingSubstateStore<TTx>,
        diff: &tari_engine_types::substate::SubstateDiff,
    ) -> Option<Refusal> {
        let max_bytes = self.max_proposal_shard_output_bytes?;
        let total_bytes = substate_store.diff_shard_output_bytes_over(diff, max_bytes)?;
        Some(NoVoteReason::BlockShardOutputBytesExceeded { total_bytes, max_bytes }.into())
    }

    fn execute_transaction<TTx: StateStoreReadTransaction>(
        &self,
        substate_store: &PendingSubstateStore<TTx>,
        locked_epoch: LockedEpoch,
        transaction: TransactionRecord,
        change_set: &ProposedBlockChangeSet,
    ) -> Result<TransactionExecution, HotStuffError> {
        let tx = substate_store.read_transaction();
        // The proposer of this block, or a block before it in the chain, may have executed it already
        if let Some(execution) =
            BlockTransactionExecution::get_pending_for_block(tx, transaction.id(), &self.chain_tip).optional()?
        {
            return Ok(execution.into_transaction_execution());
        }

        let mut pledged = PledgedTransaction::load_pledges(tx, &self.chain_tip, transaction)?;
        let transaction_id = *pledged.id();
        pledged
            .foreign_pledges
            .extend(change_set.get_foreign_pledges(&transaction_id).cloned());

        info!(
            target: LOG_TARGET,
            "👨‍🔧 Executing transaction {transaction_id} in {} (pledges: {} local, {} foreign)",
            self.chain_tip,
            pledged.local_pledges.len(),
            pledged.foreign_pledges.len(),
        );

        self.transaction_manager
            .execute(
                substate_store,
                self.local_committee_info,
                locked_epoch,
                self.epoch,
                pledged,
            )
            .map_err(|e| HotStuffError::TransactionExecutorError(e.to_string()))
    }
}

/// The stage `command` moves its transaction to.
fn next_stage(command: &Command) -> Result<TransactionPoolStage, HotStuffError> {
    match command {
        Command::LocalOnly(_) => Ok(TransactionPoolStage::LocalOnly),
        Command::LocalPrepare(_) => Ok(TransactionPoolStage::LocalPrepared),
        Command::LocalAccept(_) => Ok(TransactionPoolStage::LocalAccepted),
        Command::AllAccept(_) => Ok(TransactionPoolStage::AllAccepted),
        Command::SomeAccept(_) => Ok(TransactionPoolStage::SomeAccepted),
        command => Err(HotStuffError::InvariantError(format!(
            "Command {command} does not move a transaction between stages"
        ))),
    }
}

/// Consensus rule: a leader may only move a transaction out of its current stage once the shard groups involved in it
/// are ready for that. Evidence equality with the atom cannot stand in for this, because every replica derives the
/// same incomplete evidence.
pub(super) fn check_ready_to_leave_stage(
    pool_record: &TransactionPoolRecord,
    local_shard_group: ShardGroup,
    command: &'static str,
) -> Option<NoVoteReason> {
    if pool_record.is_ready_for_pending_stage(local_shard_group) {
        return None;
    }
    Some(NoVoteReason::ProposedBeforeShardGroupsReady {
        transaction_id: *pool_record.id(),
        stage: pool_record.current_stage(),
        command,
    })
}

/// The state Merkle root of a block whose commands left `substate_store`, after the block pays `total_leader_fee` to
/// its proposer, with the pending tree diffs it produces.
pub(super) fn block_state_root<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    local_committee_info: &CommitteeInfo,
    parent_block_id: &tari_consensus_types::BlockId,
    substate_store: &mut PendingSubstateStore<TTx>,
    proposer_claim_public_key: &RistrettoPublicKeyBytes,
    total_leader_fee: u64,
    network: Network,
    epoch: Epoch,
) -> Result<(FixedHash, IndexMap<Shard, PendingShardStateTreeDiff>), HotStuffError> {
    let shard_group = local_committee_info.shard_group();
    if total_leader_fee > 0 {
        apply_leader_fee_to_substate_store(
            substate_store,
            proposer_claim_public_key,
            local_committee_info.num_preshards(),
            shard_group.start(),
            total_leader_fee,
        )?;
    }
    let pending: HashMap<_, _> = PendingShardStateTreeDiff::get_all_up_to_commit_block(tx, parent_block_id)?;
    let (root, diffs) = calculate_state_merkle_root(
        tx,
        shard_group,
        pending,
        substate_store
            .changes()
            .iter()
            // Local shards and the global shard only
            .filter(|ch| shard_group.contains_or_global(&ch.shard())),
        network,
        epoch,
    )?;
    Ok((root, diffs))
}
