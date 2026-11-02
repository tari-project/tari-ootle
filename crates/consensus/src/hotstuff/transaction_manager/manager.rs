//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    marker::PhantomData,
};

use indexmap::{IndexMap, IndexSet};
use log::*;
use tari_consensus_types::{Decision, LeafBlock};
use tari_engine_types::{
    commit_result::{AbortReason, RejectReason},
    creation_check::{CreationCheck, CreationCheckError},
    substate::{Substate, SubstateId},
};
use tari_ootle_common_types::{
    Epoch,
    InputDeclarationRef,
    LockIntent,
    SubstateRequirement,
    SubstateVersion,
    VersionedSubstateIdRef,
    committee::CommitteeInfo,
    optional::{IsNotFoundError, Optional},
};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    consensus_models::{
        BlockTransactionExecution,
        Evidence,
        LockedEpoch,
        SubstatePledge,
        SubstateRequirementLockIntent,
        TransactionExecution,
        TransactionPoolRecord,
        TransactionRecord,
        VersionedSubstateIdLockIntent,
    },
};
use tari_ootle_transaction::{Transaction, TransactionId};

use super::{PledgedTransaction, PreparedTransaction, UncommittedTemplateStore};
use crate::{
    hotstuff::{
        block_change_set::ProposedBlockChangeSet,
        substate_store::{InputPledge, LockFailedError, LockStatus, PendingSubstateStore, SubstateStoreError},
    },
    tracing::TraceTimer,
    traits::{BlockTransactionExecutor, BlockTransactionExecutorError, ReadableSubstateStore},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::block_transaction_executor";

#[derive(Debug, Clone)]
pub struct ConsensusTransactionManager<TExecutor, TStateStore> {
    executor: TExecutor,
    uncommitted_templates: UncommittedTemplateStore,
    max_transaction_validity_epochs: u64,
    max_transaction_pledge_bytes: usize,
    _store: PhantomData<TStateStore>,
}

impl<TStateStore: StateStore, TExecutor: BlockTransactionExecutor<TStateStore>>
    ConsensusTransactionManager<TExecutor, TStateStore>
{
    pub fn new(executor: TExecutor, max_transaction_validity_epochs: u64, max_transaction_pledge_bytes: usize) -> Self {
        Self {
            executor,
            uncommitted_templates: UncommittedTemplateStore::default(),
            max_transaction_validity_epochs,
            max_transaction_pledge_bytes,
            _store: PhantomData,
        }
    }

    pub fn uncommitted_templates(&self) -> &UncommittedTemplateStore {
        &self.uncommitted_templates
    }

    /// Adds the transaction's local inputs to what the block pledges to foreign shard groups, before they are
    /// locked. Returns what the transaction is prepared as instead if they cannot be pledged: an abort if they are
    /// more than any block may pledge, or a deferral if they only do not fit in this block. Only the shard group
    /// holding the inputs measures them, so it aborts unilaterally and the other shard groups adopt the abort from
    /// its foreign proposal.
    fn pledge_local_inputs<'i, TTx: StateStoreReadTransaction, I>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        transaction_id: &TransactionId,
        inputs: I,
    ) -> Result<Option<PreparedTransaction>, BlockTransactionExecutorError>
    where
        I: IntoIterator<Item = VersionedSubstateIdRef<'i>>,
    {
        match store.pledge_inputs(inputs, self.max_transaction_pledge_bytes)? {
            InputPledge::Pledged => Ok(None),
            InputPledge::TransactionTooLarge { bytes } => {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ PREPARE: Transaction {transaction_id} pledges {bytes} bytes of local inputs, more than the {} a \
                     block may pledge. Aborting.",
                    self.max_transaction_pledge_bytes,
                );
                let execution = TransactionExecution::abort(transaction_id, RejectReason::Abort {
                    reason: AbortReason::InputPledgeTooLarge,
                });
                Ok(Some(PreparedTransaction::new_multishard_executed(
                    execution,
                    LockStatus::new(),
                )))
            },
            InputPledge::BlockBudgetReached { total_bytes, max_bytes } => {
                Ok(Some(PreparedTransaction::PledgeBudgetReached {
                    total_bytes,
                    max_bytes,
                }))
            },
        }
    }

    /// The abort a transaction earns for declaring a window outside the epoch it was pinned to, or
    /// `None` if the window is acceptable.
    ///
    /// Both rules are evaluated against the pinned epoch — agreed across shard groups before
    /// execution — so every node reaches the same verdict regardless of how far its own view had
    /// lagged when the transaction was admitted. An out-of-window transaction is therefore sequenced
    /// as an abort, which all shard groups adopt, rather than being dropped by whichever group
    /// happened to be behind.
    fn window_abort_reason(&self, pinned_epoch: Epoch, max_epoch: Epoch) -> Option<AbortReason> {
        if pinned_epoch > max_epoch {
            return Some(AbortReason::EpochExpired);
        }
        let latest_permitted = Epoch(
            pinned_epoch
                .as_u64()
                .saturating_add(self.max_transaction_validity_epochs),
        );
        if max_epoch > latest_permitted {
            return Some(AbortReason::ValidityWindowTooLong);
        }
        None
    }

    /// The abort `transaction` earns for executing in a block of `block_epoch` while pinned to
    /// `pinned_epoch`, or `None` if it may execute.
    fn execution_abort_reason(
        &self,
        transaction: &Transaction,
        pinned_epoch: Epoch,
        block_epoch: Epoch,
    ) -> Option<AbortReason> {
        self.window_abort_reason(pinned_epoch, transaction.max_epoch())
            .or_else(|| burn_rate_governance_abort_reason(transaction, pinned_epoch, block_epoch))
    }

    pub fn prepare<TTx: StateStoreReadTransaction>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        pool_tx: &TransactionPoolRecord,
        block: LeafBlock,
        change_set: &ProposedBlockChangeSet,
    ) -> Result<PreparedTransaction, BlockTransactionExecutorError> {
        self.prepare_inner(store, local_committee_info, pool_tx, block, change_set, false)
    }

    /// Prepares the transaction for a block this node proposes. A proposer defers a transaction with a deferrable lock
    /// conflict, so one whose declared local inputs are locked in a way no execution could lock alongside is
    /// prepared as [PreparedTransaction::InputsLocked] without being executed.
    pub fn prepare_for_proposal<TTx: StateStoreReadTransaction>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        pool_tx: &TransactionPoolRecord,
        block: LeafBlock,
        change_set: &ProposedBlockChangeSet,
    ) -> Result<PreparedTransaction, BlockTransactionExecutorError> {
        self.prepare_inner(store, local_committee_info, pool_tx, block, change_set, true)
    }

    fn prepare_inner<TTx: StateStoreReadTransaction>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        pool_tx: &TransactionPoolRecord,
        block: LeafBlock,
        change_set: &ProposedBlockChangeSet,
        is_proposing: bool,
    ) -> Result<PreparedTransaction, BlockTransactionExecutorError> {
        let _timer = TraceTimer::info(LOG_TARGET, "prepare");
        let transaction = pool_tx.get_transaction(store.read_transaction())?;
        let transaction_id = *transaction.id();

        if let Some(reason) = pool_tx.current_decision().abort_reason() {
            // CASE: A foreign proposal caused this transaction to be sequenced and immediately aborted
            warn!(target: LOG_TARGET, "⚠️ PREPARE: Transaction {transaction_id} is ABORTED before prepare: {reason}");
            let execution = self
                .fetch_execution(store, &transaction_id, &block)?
                .unwrap_or_else(|| TransactionExecution::abort(&transaction_id, RejectReason::Abort { reason }));
            return Ok(PreparedTransaction::new_multishard_executed(
                execution,
                LockStatus::new(),
            ));
        }
        let execution_epoch = pool_tx.locked_epoch().ok_or_else(|| {
            BlockTransactionExecutorError::InvariantError(format!(
                "Pool transaction {} prepare without locked epoch",
                transaction_id
            ))
        })?;

        let resolved_inputs = self.resolve_inputs(store, &transaction, local_committee_info)?;

        match resolved_inputs {
            ResolvedTransactionInputs::OnlyLocalInputs { local_versions } => {
                // Because we have only local inputs, this shard group unilaterally decides the locked epoch (though
                // output shardgroups still may abort)
                self.prepare_local_input_transaction(
                    store,
                    local_committee_info,
                    &transaction,
                    local_versions,
                    block,
                    execution_epoch.clone(),
                    is_proposing,
                )
            },
            ResolvedTransactionInputs::LocalAndForeignInputs {
                local_versions,
                non_local_inputs,
            } => self.prepare_multishard_involved_transaction(
                store,
                local_committee_info,
                &transaction,
                local_versions,
                non_local_inputs,
                block,
                execution_epoch.clone(),
                pool_tx.is_fee_intent_only(),
            ),
            ResolvedTransactionInputs::OnlyForeignInputs { non_local_inputs } => self
                .prepare_multishard_output_only_transaction(
                    store,
                    local_committee_info,
                    &transaction,
                    non_local_inputs,
                    block,
                    change_set,
                    execution_epoch.clone(),
                    pool_tx.is_fee_intent_only(),
                ),
            ResolvedTransactionInputs::OnlyTombstones => {
                // Because we have only local inputs, this shard group unilaterally decides the locked epoch (though
                // output shards may abort) TODO: the pool record needs to be updated after this
                // function returns by the caller, which is not ideal
                self.prepare_local_input_transaction(
                    store,
                    local_committee_info,
                    &transaction,
                    IndexMap::new(),
                    block,
                    execution_epoch.clone(),
                    is_proposing,
                )
            },
            ResolvedTransactionInputs::OneOrMoreLocalInputsNotFound { is_local_only, err } => {
                // Currently, this message will differ depending on which involved shard is asked.
                // e.g. local nodes will say "failed to lock inputs", foreign nodes will say "foreign shard abort"
                let execution =
                    TransactionExecution::abort(&transaction_id, RejectReason::SubstateNotFound(err.to_string()));
                if is_local_only {
                    warn!(target: LOG_TARGET, "⚠️ PREPARE: OneOrMoreInputsNotFound transaction {} local ABORT", transaction_id);
                    Ok(PreparedTransaction::new_local_early_abort(execution))
                } else {
                    warn!(target: LOG_TARGET, "⚠️ PREPARE: OneOrMoreLocalInputsNotFound transaction {} foreign ABORT", transaction_id);
                    Ok(PreparedTransaction::new_multishard_executed(
                        execution,
                        LockStatus::default(),
                    ))
                }
            },
        }
    }

    fn resolve_local_versions<'a, TTx: StateStoreReadTransaction>(
        &self,
        store: &PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        transaction: &'a Transaction,
    ) -> Result<ResolvedInputs<'a>, BlockTransactionExecutorError> {
        let mut local_inputs = IndexMap::with_capacity(transaction.num_inputs());

        let mut foreign_inputs = IndexSet::new();
        for input in transaction.all_inputs_iter() {
            if !local_committee_info.includes_substate_id(input.substate_id()) {
                foreign_inputs.insert(input);
                continue;
            }

            match input.version() {
                Some(version) => {
                    let id = input.with_version(version);
                    store.lock_assert_is_up(id)?;
                    debug!(target: LOG_TARGET, "Resolved LOCAL substate: {id}");
                    local_inputs.insert(input, version);
                },
                None => {
                    let latest = store.get_latest_version(input.substate_id())?;
                    if latest.is_down() {
                        return Err(SubstateStoreError::SubstateNotFound {
                            id: input.with_version(latest.version()).to_owned(),
                        }
                        .into());
                    }
                    debug!(target: LOG_TARGET, "Resolved LOCAL unversioned substate: {input} to version {}",latest.version());
                    local_inputs.insert(input, latest.version());
                },
            }
        }

        let num_local_tombstones = transaction
            .claim_burn_outputs_iter()
            .filter(|tombstone| local_committee_info.includes_substate_id(&SubstateId::from(*tombstone)))
            .count();

        if local_inputs.is_empty() && foreign_inputs.is_empty() && num_local_tombstones == 0 {
            // Mempool/missing transaction validations should have not sent the transaction to consensus
            warn!(target: LOG_TARGET, "⚠️ PREPARE: NEVER HAPPEN transaction {} has no inputs.", transaction.calculate_id());
            return Err(BlockTransactionExecutorError::InvariantError(format!(
                "Transaction {} has no inputs",
                transaction.calculate_id()
            )));
        }

        Ok(ResolvedInputs {
            local_inputs,
            foreign_inputs,
            num_local_tombstones,
        })
    }

    pub fn execute<TTx: StateStoreReadTransaction>(
        &self,
        store: &PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        execution_epoch: LockedEpoch,
        block_epoch: Epoch,
        pledged_transaction: PledgedTransaction,
    ) -> Result<TransactionExecution, BlockTransactionExecutorError> {
        let transaction = pledged_transaction.transaction.transaction();
        if let Some(reason) = self.execution_abort_reason(transaction, execution_epoch.epoch(), block_epoch) {
            warn!(
                target: LOG_TARGET,
                "⏰ Transaction {} cannot execute in a block of {block_epoch} ({reason}): pinned to {}, max_epoch {}",
                pledged_transaction.transaction.id(),
                execution_epoch,
                transaction.max_epoch(),
            );
            return Ok(TransactionExecution::abort(
                pledged_transaction.transaction.id(),
                RejectReason::Abort { reason },
            ));
        }

        let resolved_inputs = pledged_transaction
            .local_pledges
            .into_iter()
            .chain(pledged_transaction.foreign_pledges)
            // Exclude any output pledges
            .filter_map(|pledge| pledge.into_input())
            .map(|(id, substate)|
                {
                    let version = id.version();
                    (
                        id.into(),
                        Substate::new(version, substate),
                    )
                })
            .collect();
        let transaction = pledged_transaction.transaction.transaction();
        self.executor.execute(
            transaction,
            execution_epoch,
            input_fanout(transaction, local_committee_info),
            &resolved_inputs,
            &self.uncommitted_templates.templates_for(store)?,
            &LocalCreationCheck::new(store, local_committee_info, pledged_transaction.transaction.id()),
        )
    }

    pub fn execute_or_fetch<TTx: StateStoreReadTransaction>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        transaction: &TransactionRecord,
        resolved_inputs: &HashMap<SubstateRequirement, Substate>,
        block: &LeafBlock,
        execution_locked_epoch: LockedEpoch,
    ) -> Result<TransactionExecution, BlockTransactionExecutorError> {
        if let Some(reason) =
            self.execution_abort_reason(transaction.transaction(), execution_locked_epoch.epoch(), block.epoch)
        {
            warn!(
                target: LOG_TARGET,
                "⏰ Transaction {} cannot execute in a block of {} ({reason}): pinned to {}, max_epoch {}",
                transaction.id(),
                block.epoch,
                execution_locked_epoch.epoch(),
                transaction.transaction().max_epoch(),
            );
            return Ok(TransactionExecution::abort(transaction.id(), RejectReason::Abort {
                reason,
            }));
        }

        // Might have been executed already in on propose
        if let Some(execution) = self.fetch_execution(store, transaction.id(), block)? {
            info!(
                target: LOG_TARGET,
                "👨‍🔧 PREPARE: Found existing execution for transaction {}: {}",
                transaction.id(),
                execution
            );
            // Artifacts are stored alongside the execution
            return Ok(execution);
        }
        info!(
            target: LOG_TARGET,
            "👨‍🔧 PREPARE: Executing transaction {}",
            transaction.id(),
        );
        self.executor.execute(
            transaction.transaction(),
            execution_locked_epoch,
            input_fanout(transaction.transaction(), local_committee_info),
            resolved_inputs,
            &self.uncommitted_templates.templates_for(store)?,
            &LocalCreationCheck::new(store, local_committee_info, transaction.id()),
        )
    }

    pub fn fetch_execution<TTx: StateStoreReadTransaction>(
        &self,
        store: &PendingSubstateStore<TTx>,
        transaction_id: &TransactionId,
        block: &LeafBlock,
    ) -> Result<Option<TransactionExecution>, BlockTransactionExecutorError> {
        if let Some(execution) =
            BlockTransactionExecution::get_pending_for_block(store.read_transaction(), transaction_id, block)
                .optional()?
        {
            return Ok(Some(execution.into_transaction_execution()));
        }
        Ok(None)
    }

    fn resolve_inputs<'a, TTx: StateStoreReadTransaction>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        transaction: &'a TransactionRecord,
        local_committee_info: &CommitteeInfo,
    ) -> Result<ResolvedTransactionInputs<'a>, BlockTransactionExecutorError> {
        match self.resolve_local_versions(store, local_committee_info, transaction.transaction()) {
            Ok(resolved) => {
                if resolved.local_inputs.is_empty() &&
                    resolved.foreign_inputs.is_empty() &&
                    resolved.num_local_tombstones > 0
                {
                    // EDGE CASE: transaction with only tombstone outputs and no inputs has to be handled differently
                    return Ok(ResolvedTransactionInputs::OnlyTombstones);
                }

                if !resolved.local_inputs.is_empty() && resolved.foreign_inputs.is_empty() {
                    // EDGE CASE: global transaction, but single committee = OnlyLocalInputs behaviour, otherwise
                    // LocalAndForeignInputs behaviour
                    if transaction.transaction().is_global() && local_committee_info.num_committees() > 1 {
                        return Ok(ResolvedTransactionInputs::LocalAndForeignInputs {
                            local_versions: resolved.local_inputs,
                            non_local_inputs: resolved.foreign_inputs,
                        });
                    }

                    return Ok(ResolvedTransactionInputs::OnlyLocalInputs {
                        local_versions: resolved.local_inputs,
                    });
                }

                if resolved.local_inputs.is_empty() && !resolved.foreign_inputs.is_empty() {
                    return Ok(ResolvedTransactionInputs::OnlyForeignInputs {
                        non_local_inputs: resolved.foreign_inputs,
                    });
                }

                Ok(ResolvedTransactionInputs::LocalAndForeignInputs {
                    local_versions: resolved.local_inputs,
                    non_local_inputs: resolved.foreign_inputs,
                })
            },
            Err(err) => {
                warn!(target: LOG_TARGET, "⚠️ PREPARE: failed to resolve local inputs: {err}");
                // We only expect not found or down errors here. If we get any other error, this is fatal.
                if !err.is_not_found_error() {
                    return Err(err);
                }

                let is_local_only = local_committee_info.is_all_local(transaction.transaction.all_inputs_iter()) &&
                    local_committee_info.is_all_local(transaction.transaction.known_outputs_iter());
                Ok(ResolvedTransactionInputs::OneOrMoreLocalInputsNotFound { is_local_only, err })
            },
        }
    }

    fn prepare_local_input_transaction<TTx: StateStoreReadTransaction>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        transaction: &TransactionRecord,
        local_versions: IndexMap<InputDeclarationRef<'_>, SubstateVersion>,
        block: LeafBlock,
        execution_locked_epoch: LockedEpoch,
        is_proposing: bool,
    ) -> Result<PreparedTransaction, BlockTransactionExecutorError> {
        let transaction_id = *transaction.id();
        // CASE: All inputs are local and we can execute the transaction.
        //       Outputs may or may not be local

        info!(
            target: LOG_TARGET,
            "👨‍🔧 PREPARE: transaction {} has {} local input(s) and {} tombstone(s)",
            transaction_id,
            local_versions.len(),
            transaction.transaction().claim_burn_iter().count()
        );

        // A transaction outside its validity window aborts without executing, so it is not worth deferring.
        let max_epoch = transaction.transaction().max_epoch();
        if is_proposing &&
            self.window_abort_reason(execution_locked_epoch.epoch(), max_epoch)
                .is_none()
        {
            let lock_status = store.check_input_locks(
                &transaction_id,
                local_versions
                    .iter()
                    .map(|(decl, version)| declared_lock_intent(decl, *version)),
            )?;
            if lock_status.is_deferrable_conflict() {
                info!(
                    target: LOG_TARGET,
                    "👨‍🔧 PREPARE: transaction {} has local input(s) locked by another transaction. Not executing.",
                    transaction_id,
                );
                return Ok(PreparedTransaction::InputsLocked { lock_status });
            }
        }

        let local_inputs = store.get_many(
            local_versions
                .iter()
                .map(|(req, v)| (req.to_substate_requirement_ref().to_owned(), *v)),
        )?;
        let execution = self.execute_or_fetch(
            store,
            local_committee_info,
            transaction,
            &local_inputs,
            &block,
            execution_locked_epoch,
        )?;

        let is_outputs_local_only = local_committee_info.is_all_local(execution.involved_outputs());
        if is_outputs_local_only {
            // CASE: All inputs are local and all outputs are local
            info!(
                target: LOG_TARGET,
                "👨‍🔧 PREPARE: Local-Only Executed transaction {} with {} decision",
                transaction_id,
                execution.decision()
            );

            if let Some(reason) = execution.decision().abort_reason() {
                warn!(target: LOG_TARGET, "⚠️ PREPARE: LocalOnly Aborted: {reason}");
                return Ok(PreparedTransaction::new_local_early_abort(execution));
            }

            let (execution, lock_status) = lock_local_only_execution(store, transaction_id, execution)?;
            if let Some(err) = lock_status.hard_conflict() {
                warn!(target: LOG_TARGET, "⚠️ PREPARE: Hard conflict when locking inputs: {err}");
                let execution =
                    TransactionExecution::abort(transaction.id(), RejectReason::FailedToLockInputs(err.to_string()));
                return Ok(PreparedTransaction::new_local_accept(execution, lock_status));
            }

            return Ok(PreparedTransaction::new_local_accept(execution, lock_status));
        }

        info!(target: LOG_TARGET, "👨‍🔧 PREPARE: transaction {} has {} local input(s) and foreign outputs (Local decision: {})", transaction_id, local_inputs.len(), execution.decision());
        match execution.decision() {
            Decision::Commit => {
                // CASE: Multishard transaction, all inputs are local, consensus with output shard groups
                // pending
                let requested_locks = execution.resolved_inputs();
                if let Some(prepared) = self.pledge_local_inputs(
                    store,
                    &transaction_id,
                    requested_locks
                        .iter()
                        .filter(|lock| local_committee_info.includes_substate_id(lock.substate_id()))
                        .map(|lock| lock.to_versioned_substate_id_ref()),
                )? {
                    return Ok(prepared);
                }
                let lock_status = store.try_lock_all(transaction_id, requested_locks, false)?;
                if let Some(err) = lock_status.hard_conflict() {
                    warn!(target: LOG_TARGET, "⚠️ PREPARE: Hard conflict when locking inputs: {err}");
                    let execution = TransactionExecution::abort(
                        transaction.id(),
                        RejectReason::FailedToLockInputs(err.to_string()),
                    );
                    return Ok(PreparedTransaction::new_multishard_executed(execution, lock_status));
                }
                // We're committing, and one or more of the outputs are foreign
                Ok(PreparedTransaction::new_multishard_executed(execution, lock_status))
            },
            Decision::Abort(reason) => {
                // CASE: We're aborting, so this is a local-only transaction since no outputs need to be
                // created
                warn!(target: LOG_TARGET, "⚠️ PREPARE: Aborted: {reason:?}");
                Ok(PreparedTransaction::new_local_early_abort(execution))
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_multishard_involved_transaction<TTx: StateStoreReadTransaction>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        transaction: &TransactionRecord,
        local_versions: IndexMap<InputDeclarationRef<'_>, SubstateVersion>,
        non_local_inputs: IndexSet<InputDeclarationRef<'_>>,
        block: LeafBlock,
        execution_locked_epoch: LockedEpoch,
        fee_intent_only: bool,
    ) -> Result<PreparedTransaction, BlockTransactionExecutorError> {
        // CASE: Multishard transaction, some inputs are local, some are foreign

        info!(
            target: LOG_TARGET,
            "👨‍🔧 PREPARE: transaction {} has {} local input(s) and {} foreign input(s)",
            transaction.id(),
            local_versions.len(),
            non_local_inputs.len()
        );

        // Without every input this shard group cannot execute, so a window that is too long is rejected here, before
        // any input is locked. The LocalPrepared timeout is measured from max_epoch, so it only bounds how long locks
        // are held if max_epoch is within the window. The other shard groups receive and adopt the abort.
        // An expired max_epoch is left to execution: this group's pin may still be lowered by another group's, and
        // execution judges expiry at the final pin. Its locks are bounded by the LocalPrepared timeout meanwhile.
        // A window too long at this pin is too long at any lower pin, so that verdict cannot change.
        let max_epoch = transaction.transaction().max_epoch();
        if let Some(reason @ AbortReason::ValidityWindowTooLong) =
            self.window_abort_reason(execution_locked_epoch.epoch(), max_epoch)
        {
            warn!(
                target: LOG_TARGET,
                "⏰ PREPARE: Transaction {} declares a validity window that is too long: pinned to {}, max_epoch {max_epoch}",
                transaction.id(),
                execution_locked_epoch,
            );
            let execution = TransactionExecution::abort(transaction.id(), RejectReason::Abort { reason });
            return Ok(PreparedTransaction::new_multishard_executed(
                execution,
                LockStatus::default(),
            ));
        }

        match self.fetch_execution(store, transaction.id(), &block)? {
            Some(exec) => self.prepare_multishard_from_execution(
                store,
                local_committee_info,
                *transaction.id(),
                exec,
                fee_intent_only,
            ),
            None => self.prepare_multishard_no_execution(
                store,
                local_committee_info,
                *transaction.id(),
                local_versions,
                non_local_inputs,
            ),
        }
    }

    /// Locks the inputs and local outputs of the multishard `execution`. `fee_intent_only` is set if another shard
    /// group has found that the transaction commits only its fee intent.
    fn prepare_multishard_from_execution<TTx: StateStoreReadTransaction>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        transaction_id: TransactionId,
        execution: TransactionExecution,
        fee_intent_only: bool,
    ) -> Result<PreparedTransaction, BlockTransactionExecutorError> {
        if let Some(reason) = execution.decision().abort_reason() {
            warn!(target: LOG_TARGET, "⚠️ PREPARE: Transaction {transaction_id} is ABORTED before prepare: {reason}");
            // No locks
            let lock_status = LockStatus::default();
            return Ok(PreparedTransaction::new_multishard_executed(execution, lock_status));
        }

        if let Some(prepared) = self.pledge_local_inputs(
            store,
            &transaction_id,
            execution
                .resolved_inputs()
                .iter()
                .filter(|lock| local_committee_info.includes_substate_id(lock.substate_id()))
                .map(|lock| lock.to_versioned_substate_id_ref()),
        )? {
            return Ok(prepared);
        }

        let locked = lock_multishard_execution(
            store,
            local_committee_info,
            transaction_id,
            execution,
            MultishardLockScope::Execution,
            fee_intent_only,
        )?;
        if let Some(err) = locked.lock_status.hard_conflict() {
            // CASE: Multishard transaction, will abort due to hard lock conflict
            warn!(target: LOG_TARGET, "⚠️ PREPARE: Hard conflict when locking inputs: {err}");
            let execution = locked
                .execution
                .into_abort(&transaction_id, RejectReason::FailedToLockInputs(err.to_string()));
            Ok(PreparedTransaction::new_multishard_executed(
                execution,
                locked.lock_status,
            ))
        } else {
            // CASE: Multishard transaction, executed, no or "soft" (unversioned) lock conflict
            Ok(PreparedTransaction::new_multishard_locked(locked))
        }
    }

    fn prepare_multishard_no_execution<TTx: StateStoreReadTransaction>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        transaction_id: TransactionId,
        local_versions: IndexMap<InputDeclarationRef<'_>, SubstateVersion>,
        non_local_inputs: IndexSet<InputDeclarationRef<'_>>,
    ) -> Result<PreparedTransaction, BlockTransactionExecutorError> {
        // This shard group does not hold every input, so it cannot execute to discover how each one is
        // used and must lock on what the transaction declared.
        let requested_locks = local_versions
            .iter()
            .map(|(decl, version)| {
                let substate = store.get(VersionedSubstateIdRef::new(decl.substate_id(), *version))?;
                Ok(declared_lock_intent(decl, *version).with_pledged_value(substate.substate_value()))
            })
            .collect::<Result<Vec<_>, SubstateStoreError>>()?;

        if let Some(prepared) = self.pledge_local_inputs(
            store,
            &transaction_id,
            requested_locks.iter().map(|lock| lock.to_versioned_substate_id_ref()),
        )? {
            return Ok(prepared);
        }

        let mut evidence = Evidence::from_lock_intents(
            local_committee_info.num_preshards(),
            local_committee_info.num_committees(),
            &requested_locks,
        );
        // Add unpledged foreign input evidence
        for input in non_local_inputs {
            evidence.insert_unpledged_from_substate_id(
                local_committee_info.num_preshards(),
                local_committee_info.num_committees(),
                input.substate_id().clone(),
            );
        }

        // Pledge the local inputs
        let lock_status = store.try_lock_all(transaction_id, &requested_locks, false)?;
        info!(
            target: LOG_TARGET,
            "👨‍🔧 PREPARE: Multishard transaction {transaction_id} requires additional input pledges. Partial evidence: {evidence}",
        );
        Ok(PreparedTransaction::new_multishard_evidence(evidence, lock_status))
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_multishard_output_only_transaction<TTx: StateStoreReadTransaction>(
        &self,
        store: &mut PendingSubstateStore<TTx>,
        local_committee_info: &CommitteeInfo,
        transaction: &TransactionRecord,
        non_local_inputs: IndexSet<InputDeclarationRef<'_>>,
        block: LeafBlock,
        change_set: &ProposedBlockChangeSet,
        execution_locked_epoch: LockedEpoch,
        fee_intent_only: bool,
    ) -> Result<PreparedTransaction, BlockTransactionExecutorError> {
        let transaction_id = *transaction.id();
        // We're output-only, so only need to gather foreign pledges
        let foreign_pledges = transaction.get_foreign_pledges(store.read_transaction())?;

        info!(
            target: LOG_TARGET,
            "👨‍🔧 PREPARE: Transaction {} is output-only with {} foreign pledge(s)",
            transaction_id,
            foreign_pledges.len()
        );

        let proposed_foreign_pledges = change_set.get_foreign_pledges(&transaction_id);
        let resolved_inputs = resolve_foreign_input_pledges(
            &transaction_id,
            foreign_pledges.into_iter().chain(proposed_foreign_pledges.cloned()),
        )?;

        // Check that we have all the required foreign input pledges
        if let Some(req) = non_local_inputs
            .iter()
            .find(|req| !resolved_inputs.contains_key(req.substate_id()))
        {
            error!(target: LOG_TARGET, "⚠️ PREPARE: Missing {req} foreign input pledge for transaction {}", transaction_id);
            // NOTE: crashes consensus. Proposers and replicas only prepare a New transaction once it is ready, and an
            // output-only transaction is ready only once it holds every foreign input pledge.
            return Err(BlockTransactionExecutorError::InvariantError(format!(
                "Missing foreign input pledge for transaction {}: {}",
                transaction_id, req
            )));
        }

        let execution = self.execute_or_fetch(
            store,
            local_committee_info,
            transaction,
            &resolved_inputs,
            &block,
            execution_locked_epoch,
        )?;
        info!(
            target: LOG_TARGET,
            "👨‍🔧 PREPARE: Output-Only Executed transaction {} with {} decision",
            transaction_id,
            execution.decision()
        );

        self.prepare_multishard_from_execution(store, local_committee_info, transaction_id, execution, fee_intent_only)
    }
}

enum ResolvedTransactionInputs<'a> {
    OnlyLocalInputs {
        local_versions: IndexMap<InputDeclarationRef<'a>, SubstateVersion>,
    },
    LocalAndForeignInputs {
        local_versions: IndexMap<InputDeclarationRef<'a>, SubstateVersion>,
        non_local_inputs: IndexSet<InputDeclarationRef<'a>>,
    },
    /// Does not involve any local inputs, but involves one or more local tombstone outputs
    OnlyTombstones,
    /// a.k.a "output-only"
    OnlyForeignInputs {
        non_local_inputs: IndexSet<InputDeclarationRef<'a>>,
    },
    OneOrMoreLocalInputsNotFound {
        is_local_only: bool,
        err: BlockTransactionExecutorError,
    },
}

struct ResolvedInputs<'a> {
    pub local_inputs: IndexMap<InputDeclarationRef<'a>, SubstateVersion>,
    pub foreign_inputs: IndexSet<InputDeclarationRef<'a>>,
    pub num_local_tombstones: usize,
}

/// Answers for the substates in the local shard group as the block being evaluated sees them, with the same rules
/// [`is_output_never_creatable`] applies when locking. Every node evaluating the block evaluates its transactions in
/// the same order against the same store, so every node gets the same answers. A substate held by another shard group
/// is reported as not taken, since this store does not hold it: the group holding it rejects the transaction, and an
/// abort from any shard group aborts it everywhere.
struct LocalCreationCheck<'a, 'tx, TTx: StateStoreReadTransaction> {
    store: &'a PendingSubstateStore<'tx, TTx>,
    local_committee_info: &'a CommitteeInfo,
    transaction_id: &'a TransactionId,
}

impl<'a, 'tx, TTx: StateStoreReadTransaction> LocalCreationCheck<'a, 'tx, TTx> {
    fn new(
        store: &'a PendingSubstateStore<'tx, TTx>,
        local_committee_info: &'a CommitteeInfo,
        transaction_id: &'a TransactionId,
    ) -> Self {
        Self {
            store,
            local_committee_info,
            transaction_id,
        }
    }
}

impl<TTx: StateStoreReadTransaction> CreationCheck for LocalCreationCheck<'_, '_, TTx> {
    fn is_creation_taken(&self, id: &SubstateId) -> Result<bool, CreationCheckError> {
        if !self.local_committee_info.includes_substate_id(id) {
            return Ok(false);
        }
        self.store
            .is_creation_taken(self.transaction_id, id)
            .map_err(|err| CreationCheckError {
                id: id.clone(),
                details: err.to_string(),
            })
    }
}

/// Locks the inputs and outputs of `execution`, a transaction whose inputs and outputs are all local. A transaction
/// that cannot create one of its outputs in this or any later block still commits its fee intent, which pays for its
/// execution, and that is what is locked and returned instead.
fn lock_local_only_execution<TTx: StateStoreReadTransaction>(
    store: &mut PendingSubstateStore<TTx>,
    transaction_id: TransactionId,
    mut execution: TransactionExecution,
) -> Result<(TransactionExecution, LockStatus), BlockTransactionExecutorError> {
    let lock_status = lock_all_or_none(store, transaction_id, &execution)?;
    let Some(err) = lock_status.hard_conflict() else {
        return Ok((execution, lock_status));
    };
    if !is_output_never_creatable(store, err)? {
        return Ok((execution, lock_status));
    }
    let reason = RejectReason::FailedToLockOutputs(err.to_string());
    let Some(fee_intent_commit) = execution.take_fee_intent_commit(reason) else {
        return Ok((execution, lock_status));
    };
    warn!(
        target: LOG_TARGET,
        "⚠️ PREPARE: Transaction {transaction_id} creates an output that exists or is being created by another \
         transaction. Committing its fee intent only",
    );
    let lock_status = lock_all_or_none(store, transaction_id, &fee_intent_commit)?;
    Ok((fee_intent_commit, lock_status))
}

/// Locks every input and output of the local-only `execution`, or none of them. A transaction that is aborted or
/// deferred holds no locks, so a lock held under local-only rules belongs to a transaction this block or one of
/// its ancestors commits.
fn lock_all_or_none<TTx: StateStoreReadTransaction>(
    store: &mut PendingSubstateStore<TTx>,
    transaction_id: TransactionId,
    execution: &TransactionExecution,
) -> Result<LockStatus, BlockTransactionExecutorError> {
    let requested_locks = execution.resolved_inputs().iter().chain(execution.resulting_outputs());
    let lock_status = store.try_lock_all(transaction_id, requested_locks, true)?;
    if lock_status.is_any_failed() {
        store.release_new_locks(&transaction_id);
    }
    Ok(lock_status)
}

/// Whether `err` means an output version cannot be created in this or any later block on this chain: the version
/// already exists, or a local-only transaction that this chain commits creates it or a later version.
fn is_output_never_creatable<TTx: StateStoreReadTransaction>(
    store: &PendingSubstateStore<TTx>,
    err: &LockFailedError,
) -> Result<bool, BlockTransactionExecutorError> {
    match err {
        LockFailedError::SubstateExists { .. } => Ok(true),
        LockFailedError::LockConflict { substate_id, conflict } => {
            if !conflict.requested_lock.is_output() || !conflict.existing_lock.is_output() {
                return Ok(false);
            }
            let existing = store.get_latest_lock_by_id(substate_id.substate_id())?;
            Ok(existing.is_some_and(|lock| lock.is_local_only() && lock.version() >= substate_id.version()))
        },
        LockFailedError::SubstateNotFound { .. } => Ok(false),
    }
}

/// The locks a shard group takes for a multishard execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultishardLockScope {
    /// The local inputs, the local outputs and the receipt, taken by a shard group that executes at the New stage.
    Execution,
    /// The local outputs, taken at LocalAccept by a shard group that locked its inputs when it prepared.
    LocalOutputs,
}

impl MultishardLockScope {
    fn locks<'a>(
        self,
        execution: &'a TransactionExecution,
        local_committee_info: &CommitteeInfo,
    ) -> Vec<&'a VersionedSubstateIdLockIntent> {
        match self {
            Self::Execution => execution
                .resolved_inputs()
                .iter()
                .chain(execution.resulting_outputs())
                .filter(|lock| {
                    lock.substate_id().is_transaction_receipt() ||
                        local_committee_info.includes_substate_id(lock.substate_id())
                })
                .collect(),
            Self::LocalOutputs => execution
                .resulting_outputs()
                .iter()
                .filter(|lock| local_committee_info.includes_substate_id(lock.substate_id()))
                .collect(),
        }
    }
}

/// A multishard execution and the locks taken for it in the local shard group.
#[derive(Debug)]
pub struct LockedMultishardExecution {
    pub execution: TransactionExecution,
    pub lock_status: LockStatus,
    /// The execution commits only the transaction's fee intent. See [`MultiShardAtom::fee_intent_only`].
    pub fee_intent_only: bool,
}

/// Takes the `scope` locks for the multishard `execution`. A transaction that cannot create one of its local outputs
/// in this or any later block commits only its fee intent in every shard group, so its fee-intent commit is locked and
/// returned instead. If another shard group has already found that (`fee_intent_only`), or `execution` is the
/// fee-intent commit already, the fee-intent commit is locked from the start.
pub fn lock_multishard_execution<TTx: StateStoreReadTransaction>(
    store: &mut PendingSubstateStore<TTx>,
    local_committee_info: &CommitteeInfo,
    transaction_id: TransactionId,
    execution: TransactionExecution,
    scope: MultishardLockScope,
    fee_intent_only: bool,
) -> Result<LockedMultishardExecution, BlockTransactionExecutorError> {
    let mut execution = if fee_intent_only {
        adopt_fee_intent_commit(&transaction_id, execution)?
    } else {
        execution
    };
    // The execution the proposer stored for the block may already be the fee-intent commit
    let fee_intent_only = fee_intent_only || execution.is_fee_intent_commit;
    let lock_status = store.try_lock_all(transaction_id, scope.locks(&execution, local_committee_info), false)?;
    let conflict = match lock_status.hard_conflict() {
        Some(err) if !fee_intent_only && is_output_never_creatable(store, err)? => err.to_string(),
        _ => {
            return Ok(LockedMultishardExecution {
                execution,
                lock_status,
                fee_intent_only,
            });
        },
    };
    let Some(fee_intent_commit) = take_multishard_fee_intent_commit(&mut execution) else {
        return Ok(LockedMultishardExecution {
            execution,
            lock_status,
            fee_intent_only,
        });
    };
    warn!(
        target: LOG_TARGET,
        "⚠️ Multishard transaction {transaction_id} creates an output that exists or is being created by another \
         transaction ({conflict}). Every shard group commits its fee intent only",
    );
    store.release_new_locks(&transaction_id);
    let lock_status = store.try_lock_all(
        transaction_id,
        scope.locks(&fee_intent_commit, local_committee_info),
        false,
    )?;
    Ok(LockedMultishardExecution {
        execution: fee_intent_commit,
        lock_status,
        fee_intent_only: true,
    })
}

/// The execution this shard group commits once another shard group has found that the transaction commits only its
/// fee intent: the fee-intent commit of `execution`. An execution that already commits only its fee intent is kept,
/// and so is an aborting one, since an abort from any shard group aborts the transaction everywhere.
pub fn adopt_fee_intent_commit(
    transaction_id: &TransactionId,
    mut execution: TransactionExecution,
) -> Result<TransactionExecution, BlockTransactionExecutorError> {
    if execution.decision().is_abort() || execution.result().finalize.is_fee_only() {
        return Ok(execution);
    }
    take_multishard_fee_intent_commit(&mut execution).ok_or_else(|| {
        BlockTransactionExecutorError::InvariantError(format!(
            "Transaction {transaction_id} has no fee intent to commit, but another shard group committed only its fee \
             intent"
        ))
    })
}

/// The fee-intent commit of the multishard `execution`. `None` if there is no fee intent to commit, or if the fee
/// intent creates a substate that the full execution does not: a shard group that adopts the fee-intent commit after
/// accepting the full execution holds locks only for the full execution's outputs. Every shard group executes the
/// transaction alike, so each one reaches the same answer. The reject reason names no substate, because only the
/// shard group holding the output knows which one it is, and every shard group must report the same result.
fn take_multishard_fee_intent_commit(execution: &mut TransactionExecution) -> Option<TransactionExecution> {
    let reason = RejectReason::FailedToLockOutputs(
        "one of the transaction's outputs exists or is being created by another transaction".to_string(),
    );
    let fee_intent_commit = execution.take_fee_intent_commit(reason)?;
    let full_outputs = execution
        .resulting_outputs()
        .iter()
        .map(|output| output.to_versioned_substate_id_ref())
        .collect::<HashSet<_>>();
    fee_intent_commit
        .resulting_outputs()
        .iter()
        .all(|output| full_outputs.contains(&output.to_versioned_substate_id_ref()))
        .then_some(fee_intent_commit)
}

/// Collects the input pledges among `pledges` into the inputs a transaction executes against, excluding output
/// pledges. Every pledge for a substate must be at the same version. A pledge's value is authenticated against its
/// evidence's value hash when the foreign proposal is accepted (`BlockPledge::get_committed_pledge`), so only the
/// version is compared here.
fn resolve_foreign_input_pledges(
    transaction_id: &TransactionId,
    pledges: impl IntoIterator<Item = SubstatePledge>,
) -> Result<HashMap<SubstateRequirement, Substate>, BlockTransactionExecutorError> {
    let mut resolved_inputs = HashMap::new();
    // Exclude any output pledges
    for (id, substate) in pledges.into_iter().filter_map(|pledge| pledge.into_input()) {
        let version = id.version();
        match resolved_inputs.entry(SubstateRequirement::from(id)) {
            Entry::Occupied(entry) => {
                let existing: &Substate = entry.get();
                if existing.version() != version {
                    return Err(BlockTransactionExecutorError::InvariantError(format!(
                        "Foreign pledges for transaction {transaction_id} pledge substate {} at both v{} and \
                         v{version}",
                        entry.key().substate_id(),
                        existing.version(),
                    )));
                }
            },
            Entry::Vacant(entry) => {
                entry.insert(Substate::new(version, substate));
            },
        }
    }
    Ok(resolved_inputs)
}

/// The lock a shard group takes on an input it holds but cannot execute against, from the transaction's
/// declaration alone. Writing to a read-declared input aborts in the engine, so a read lock cannot be escaped by
/// the transaction that asked for it. A substate that no transaction can write takes a read lock whatever was
/// declared, so that the default write declaration does not serialise every transaction that uses it.
fn declared_lock_intent(decl: &InputDeclarationRef<'_>, version: SubstateVersion) -> SubstateRequirementLockIntent {
    let requirement = decl.to_substate_requirement_ref().to_owned();
    if decl.is_write() && !decl.substate_id().is_read_only() {
        SubstateRequirementLockIntent::write(requirement, version)
    } else {
        SubstateRequirementLockIntent::read(requirement, version)
    }
}

/// [`Transaction::input_fanout`] under the shard layout of the block executing the transaction.
fn input_fanout(transaction: &Transaction, local_committee_info: &CommitteeInfo) -> u32 {
    transaction.input_fanout(
        local_committee_info.num_preshards(),
        local_committee_info.num_committees(),
    )
}

/// The abort a transaction declaring the burn rate governance component earns for executing in a
/// later epoch than the one it was pinned to, or `None` if it may execute.
///
/// The template holds an activation to a lead time measured from `Consensus::current_epoch()`, which
/// is the pinned epoch. Consensus resolves an epoch's rate from committed state on the premise that
/// the lead time runs from the epoch the call actually executes in, so the two epochs must be the
/// same. Every shard group holds the component and executes the call, and an abort from any one of
/// them aborts it everywhere.
fn burn_rate_governance_abort_reason(
    transaction: &Transaction,
    pinned_epoch: Epoch,
    block_epoch: Epoch,
) -> Option<AbortReason> {
    if block_epoch == pinned_epoch {
        return None;
    }
    transaction
        .all_inputs_substate_ids_iter()
        .any(|id| id.is_burn_rate_governance())
        .then_some(AbortReason::GovernanceCallAfterPinnedEpoch)
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::PrivateKey;
    use tari_consensus_types::BlockId;
    use tari_engine_types::{
        commit_result::{ExecuteResult, FeeIntentFallback, FinalizeResult, TransactionResult},
        fees::FeeReceipt,
        non_fungible::NonFungibleContainer,
        substate::{SubstateDiff, SubstateId, SubstateValue},
    };
    use tari_ootle_common_types::{
        InputDeclarationRef,
        NumPreshards,
        ShardGroup,
        SubstateLockType,
        SubstateRequirement,
        SubstateVersion,
        VersionedSubstateId,
        VotePower,
        shard::Shard,
    };
    use tari_ootle_storage::consensus_models::{
        Block,
        LockedEpoch,
        SubstateChange,
        SubstatePledge,
        TransactionRecord,
        VersionedSubstateIdLockIntent,
    };
    use tari_ootle_transaction::{Network, Transaction, TransactionId};
    use tari_state_store_rocksdb::RocksDbStateStore;
    use tari_template_lib_types::{
        ComponentAddress,
        ObjectKey,
        constants::{
            BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS,
            PUBLIC_IDENTITY_RESOURCE_ADDRESS,
            STEALTH_TARI_RESOURCE_ADDRESS,
        },
    };

    use super::*;
    use crate::{
        hotstuff::transaction_manager::{EvidenceOrExecution, LocalPreparedTransaction},
        traits::{UncommittedChainTemplates, WriteableSubstateStore},
    };

    /// Stands in for the engine: any transaction that reaches it fails, so a test can tell an
    /// execution from an abort consensus decided before executing.
    struct RefusingExecutor;

    impl BlockTransactionExecutor<RocksDbStateStore<String>> for RefusingExecutor {
        fn execute(
            &self,
            _transaction: &Transaction,
            _locked_epoch: LockedEpoch,
            _input_fanout: u32,
            _resolved_inputs: &HashMap<SubstateRequirement, Substate>,
            _templates: &UncommittedChainTemplates,
            _creation_check: &dyn CreationCheck,
        ) -> Result<TransactionExecution, BlockTransactionExecutorError> {
            Err(BlockTransactionExecutorError::ExecutionThreadFailure(
                "reached the engine".to_string(),
            ))
        }
    }

    fn manager() -> ConsensusTransactionManager<RefusingExecutor, RocksDbStateStore<String>> {
        ConsensusTransactionManager::new(RefusingExecutor, 2160, usize::MAX)
    }

    fn calling(component: ComponentAddress) -> PledgedTransaction {
        let transaction = Transaction::builder_localnet(Epoch(100))
            .add_input(SubstateRequirement::unversioned(component))
            .build_and_seal(&PrivateKey::from(1u64));
        PledgedTransaction {
            transaction: TransactionRecord::new(transaction),
            foreign_pledges: Vec::new(),
            local_pledges: Vec::new(),
        }
    }

    fn pinned_to(epoch: u64) -> LockedEpoch {
        LockedEpoch::new(Epoch(epoch), Default::default(), Default::default())
    }

    /// The committee of a network with one shard group, in `epoch`.
    fn single_committee(epoch: Epoch) -> CommitteeInfo {
        CommitteeInfo::new(
            NumPreshards::P256,
            1,
            1,
            ShardGroup::all_shards(NumPreshards::P256),
            epoch,
            VotePower::of(1),
        )
    }

    /// Executes `pledged` in a block of `block_epoch` that builds on an empty chain.
    fn execute_in_block_of(
        execution_epoch: LockedEpoch,
        block_epoch: Epoch,
        pledged: PledgedTransaction,
    ) -> Result<TransactionExecution, BlockTransactionExecutorError> {
        let dir = tempfile::tempdir().unwrap();
        let store = RocksDbStateStore::<String>::open(dir.path().join("db"), Default::default()).unwrap();
        let block = LeafBlock {
            block_id: BlockId::zero(),
            height: Default::default(),
            epoch: block_epoch,
            shard_group: ShardGroup::all_shards(NumPreshards::P256),
        };
        store.with_read_tx(|tx| {
            let substate_store = PendingSubstateStore::init(tx, block, NumPreshards::P256)?;
            manager().execute(
                &substate_store,
                &single_committee(block_epoch),
                execution_epoch,
                block_epoch,
                pledged,
            )
        })
    }

    #[test]
    fn a_governance_call_executing_after_its_pinned_epoch_aborts() {
        let execution = execute_in_block_of(pinned_to(5), Epoch(6), calling(BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS))
            .expect("aborts before reaching the engine");
        assert_eq!(
            execution.decision().abort_reason(),
            Some(AbortReason::GovernanceCallAfterPinnedEpoch)
        );
    }

    #[test]
    fn a_governance_call_prepared_after_its_pinned_epoch_aborts() {
        let dir = tempfile::tempdir().unwrap();
        let store = RocksDbStateStore::<String>::open(dir.path().join("db"), Default::default()).unwrap();
        let block = LeafBlock {
            block_id: BlockId::zero(),
            height: Default::default(),
            epoch: Epoch(6),
            shard_group: ShardGroup::all_shards(NumPreshards::P256),
        };
        let pledged = calling(BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS);

        let execution = store
            .with_read_tx(|tx| {
                let mut substate_store = PendingSubstateStore::init(tx, block, NumPreshards::P256)?;
                manager().execute_or_fetch(
                    &mut substate_store,
                    &single_committee(Epoch(6)),
                    &pledged.transaction,
                    &HashMap::new(),
                    &block,
                    pinned_to(5),
                )
            })
            .expect("aborts before reaching the engine");
        assert_eq!(
            execution.decision().abort_reason(),
            Some(AbortReason::GovernanceCallAfterPinnedEpoch)
        );
    }

    #[test]
    fn a_governance_call_executing_in_its_pinned_epoch_executes() {
        let result = execute_in_block_of(pinned_to(5), Epoch(5), calling(BURN_RATE_GOVERNANCE_COMPONENT_ADDRESS));
        assert!(matches!(
            result,
            Err(BlockTransactionExecutorError::ExecutionThreadFailure(_))
        ));
    }

    #[test]
    fn an_ordinary_call_executing_after_its_pinned_epoch_executes() {
        let component = ComponentAddress::new(ObjectKey::from_array([1; 32]));
        let result = execute_in_block_of(pinned_to(5), Epoch(6), calling(component));
        assert!(matches!(
            result,
            Err(BlockTransactionExecutorError::ExecutionThreadFailure(_))
        ));
    }

    fn lock_type(decl: InputDeclarationRef<'_>) -> SubstateLockType {
        declared_lock_intent(&decl, SubstateVersion::ZERO).lock_type()
    }

    #[test]
    fn a_write_declaration_on_a_writable_substate_takes_a_write_lock() {
        let id = SubstateId::Component(ComponentAddress::new(ObjectKey::from_array([1; 32])));
        assert_eq!(lock_type(InputDeclarationRef::write(&id)), SubstateLockType::Write);
        assert_eq!(lock_type(InputDeclarationRef::read(&id)), SubstateLockType::Read);
    }

    #[test]
    fn a_write_declaration_on_a_read_only_substate_takes_a_read_lock() {
        for id in [
            SubstateId::Resource(STEALTH_TARI_RESOURCE_ADDRESS),
            SubstateId::Resource(PUBLIC_IDENTITY_RESOURCE_ADDRESS),
        ] {
            assert!(id.is_read_only());
            assert_eq!(lock_type(InputDeclarationRef::write(&id)), SubstateLockType::Read);
        }
    }

    fn component_id(n: u8) -> SubstateId {
        SubstateId::Component(ComponentAddress::new(ObjectKey::from_array([n; 32])))
    }

    fn input_pledge(id: &SubstateId, version: u64) -> SubstatePledge {
        SubstatePledge::Input {
            substate_id: VersionedSubstateId::new(id.clone(), SubstateVersion::new(version)),
            is_write: true,
            substate: Box::new(SubstateValue::from(NonFungibleContainer::no_data())),
        }
    }

    #[test]
    fn duplicate_input_pledges_at_the_same_version_resolve_to_one_input() {
        let id = component_id(1);
        let resolved =
            resolve_foreign_input_pledges(&TransactionId::default(), [input_pledge(&id, 3), input_pledge(&id, 3)])
                .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[&SubstateRequirement::unversioned(id)].version(),
            SubstateVersion::new(3)
        );
    }

    #[test]
    fn input_pledges_for_one_substate_at_different_versions_are_an_invariant_error() {
        let id = component_id(1);
        let err =
            resolve_foreign_input_pledges(&TransactionId::default(), [input_pledge(&id, 3), input_pledge(&id, 4)])
                .unwrap_err();
        assert!(matches!(err, BlockTransactionExecutorError::InvariantError(_)));
    }

    #[test]
    fn output_pledges_are_not_resolved_as_inputs() {
        let input = component_id(1);
        let output = component_id(2);
        let resolved = resolve_foreign_input_pledges(&TransactionId::default(), [
            input_pledge(&input, 0),
            SubstatePledge::Output {
                substate_id: VersionedSubstateId::new(output.clone(), SubstateVersion::ZERO),
            },
        ])
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert!(resolved.contains_key(&input));
        assert!(!resolved.contains_key(&output));
    }

    fn shard_of(id: &SubstateId) -> Shard {
        VersionedSubstateIdRef::new(id, SubstateVersion::ZERO).to_shard(NumPreshards::P256)
    }

    /// Prepares, pinned to epoch 1, a transaction with one input in this shard group and one in
    /// another. Returns the prepared decision and whether the local input was locked.
    fn prepare_cross_shard(max_epoch: Epoch) -> (Decision, bool) {
        let local = component_id(1);
        let local_shard = shard_of(&local);
        let foreign = (2..=u8::MAX)
            .map(component_id)
            .find(|id| shard_of(id) != local_shard)
            .unwrap();
        let committee = CommitteeInfo::new(
            NumPreshards::P256,
            1,
            2,
            ShardGroup::new(local_shard, local_shard),
            Epoch(1),
            VotePower::of(1),
        );
        let transaction = TransactionRecord::new(
            Transaction::builder_localnet(max_epoch)
                .add_input(SubstateRequirement::unversioned(local.clone()))
                .add_input(SubstateRequirement::unversioned(foreign))
                .build_and_seal(&PrivateKey::from(1u64)),
        );

        let dir = tempfile::tempdir().unwrap();
        let db = RocksDbStateStore::<String>::open(dir.path().join("db"), Default::default()).unwrap();
        let zero = Block::zero_block(Network::LocalNet, NumPreshards::P256);
        db.with_write_tx(|tx| zero.insert(tx)).unwrap();
        let block = zero.as_leaf();
        db.with_read_tx(|tx| {
            let mut store = PendingSubstateStore::init(tx, block, NumPreshards::P256)?;
            // The builder also declares the fee resource, which may share the local shard
            for input in transaction.transaction().all_inputs_iter() {
                if committee.includes_substate_id(input.substate_id()) {
                    store.put(SubstateChange::Up {
                        id: input.substate_id().clone(),
                        shard: local_shard,
                        substate: Box::new(Substate::new(SubstateVersion::ZERO, NonFungibleContainer::no_data())),
                    })?;
                }
            }
            let manager = manager();
            let ResolvedTransactionInputs::LocalAndForeignInputs {
                local_versions,
                non_local_inputs,
            } = manager.resolve_inputs(&mut store, &transaction, &committee)?
            else {
                panic!("expected local and foreign inputs");
            };
            let PreparedTransaction::MultiShard(prepared) = manager.prepare_multishard_involved_transaction(
                &mut store,
                &committee,
                &transaction,
                local_versions,
                non_local_inputs,
                block,
                pinned_to(1),
                false,
            )?
            else {
                panic!("expected a multishard preparation");
            };
            let decision = match prepared.into_evidence_or_execution() {
                EvidenceOrExecution::Execution { execution, .. } => execution.decision(),
                EvidenceOrExecution::Evidence { .. } => Decision::Commit,
            };
            Ok::<_, BlockTransactionExecutorError>((decision, store.new_locks().contains_key(&local)))
        })
        .unwrap()
    }

    #[test]
    fn a_cross_shard_transaction_outside_its_window_aborts_without_locking() {
        let (decision, is_locked) = prepare_cross_shard(Epoch(u64::MAX));
        assert_eq!(decision.abort_reason(), Some(AbortReason::ValidityWindowTooLong));
        assert!(!is_locked);
    }

    #[test]
    fn a_cross_shard_transaction_expired_at_this_pin_still_locks_its_local_input() {
        let (decision, is_locked) = prepare_cross_shard(Epoch(0));
        assert!(decision.is_commit());
        assert!(is_locked);
    }

    #[test]
    fn a_cross_shard_transaction_inside_its_window_locks_its_local_input() {
        let (decision, is_locked) = prepare_cross_shard(Epoch(2));
        assert!(decision.is_commit());
        assert!(is_locked);
    }

    /// Stands in for the engine: every transaction executes to the execution it holds.
    #[derive(Clone)]
    struct FixedExecutor(TransactionExecution);

    impl BlockTransactionExecutor<RocksDbStateStore<String>> for FixedExecutor {
        fn execute(
            &self,
            _transaction: &Transaction,
            _locked_epoch: LockedEpoch,
            _input_fanout: u32,
            _resolved_inputs: &HashMap<SubstateRequirement, Substate>,
            _templates: &UncommittedChainTemplates,
            _creation_check: &dyn CreationCheck,
        ) -> Result<TransactionExecution, BlockTransactionExecutorError> {
            Ok(self.0.clone())
        }
    }

    fn versioned(id: &SubstateId, version: u64) -> VersionedSubstateId {
        VersionedSubstateId::new(id.clone(), SubstateVersion::new(version))
    }

    fn substate(version: u64) -> Substate {
        Substate::new(SubstateVersion::new(version), NonFungibleContainer::no_data())
    }

    /// The execution of `transaction` that writes `vault` and creates `created`, settling a fee-intent fallback that
    /// writes `vault` alone if `with_fallback`.
    fn executing_to(
        transaction: &TransactionRecord,
        vault: &SubstateId,
        created: &[&SubstateId],
        with_fallback: bool,
    ) -> TransactionExecution {
        let receipt = SubstateId::TransactionReceipt(transaction.id().as_hash().into());
        let mut fee_diff = SubstateDiff::new();
        fee_diff.down(vault.clone(), SubstateVersion::ZERO);
        fee_diff.up(vault.clone(), substate(1));
        fee_diff.up(receipt.clone(), substate(0));
        let mut full_diff = fee_diff.clone();
        for id in created {
            full_diff.up((*id).clone(), substate(0));
        }
        let resulting_outputs = full_diff
            .up_iter()
            .map(|(id, substate)| {
                VersionedSubstateIdLockIntent::output(VersionedSubstateId::new(id.clone(), substate.version()))
            })
            .collect();

        TransactionExecution::new(
            ExecuteResult {
                finalize: FinalizeResult::new(
                    transaction.id().as_hash(),
                    vec![],
                    vec![],
                    TransactionResult::Accept(full_diff),
                    FeeReceipt::default(),
                ),
                execution_time: Default::default(),
                execute_epoch: Some(Epoch(1)),
                wasm_execution_points: 0,
                native_execution_points: 0,
            },
            vec![VersionedSubstateIdLockIntent::write(versioned(vault, 0), false)],
            resulting_outputs,
        )
        .with_fee_intent_fallback(with_fallback.then(|| FeeIntentFallback {
            diff: fee_diff,
            events: vec![],
            logs: vec![],
            fee_receipt: FeeReceipt::default(),
        }))
    }

    /// A local-only transaction that writes `vault` and creates `creates`, in that order, settling a fee-intent
    /// fallback that writes `vault` alone if `with_fallback`.
    struct LocalOnlySpec {
        signer: u64,
        vault: SubstateId,
        creates: Vec<SubstateId>,
        with_fallback: bool,
    }

    struct Prepared {
        execution: TransactionExecution,
        is_locked: bool,
    }

    /// Prepares `specs` in order in one block of a network of one shard group in which `existing` already exist.
    /// Returns how each was prepared, and the locks the block holds afterwards with the transaction holding each.
    fn prepare_in_one_block(
        existing: &[SubstateId],
        specs: &[LocalOnlySpec],
    ) -> (Vec<Prepared>, Vec<(TransactionId, SubstateId, SubstateLockType)>) {
        let dir = tempfile::tempdir().unwrap();
        let db = RocksDbStateStore::<String>::open(dir.path().join("db"), Default::default()).unwrap();
        let zero = Block::zero_block(Network::LocalNet, NumPreshards::P256);
        db.with_write_tx(|tx| zero.insert(tx)).unwrap();
        let block = zero.as_leaf();
        let committee = single_committee(Epoch(1));
        db.with_read_tx(|tx| {
            let mut store = PendingSubstateStore::init(tx, block, NumPreshards::P256)?;
            let transactions = specs
                .iter()
                .map(|spec| {
                    TransactionRecord::new(
                        Transaction::builder_localnet(Epoch(100))
                            .add_input(SubstateRequirement::unversioned(spec.vault.clone()))
                            .build_and_seal(&PrivateKey::from(spec.signer)),
                    )
                })
                .collect::<Vec<_>>();
            // The builder also declares the fee resource
            let mut ups = transactions
                .iter()
                .flat_map(|transaction| transaction.transaction().all_inputs_iter())
                .map(|input| input.substate_id().clone())
                .chain(existing.iter().cloned())
                .collect::<Vec<_>>();
            ups.sort();
            ups.dedup();
            for id in ups {
                store.put(SubstateChange::Up {
                    shard: shard_of(&id),
                    id,
                    substate: Box::new(substate(0)),
                })?;
            }

            let mut prepared = Vec::with_capacity(specs.len());
            for (spec, transaction) in specs.iter().zip(&transactions) {
                let creates = spec.creates.iter().collect::<Vec<_>>();
                let execution = executing_to(transaction, &spec.vault, &creates, spec.with_fallback);
                let manager = ConsensusTransactionManager::<_, RocksDbStateStore<String>>::new(
                    FixedExecutor(execution),
                    2160,
                    usize::MAX,
                );
                let ResolvedTransactionInputs::OnlyLocalInputs { local_versions } =
                    manager.resolve_inputs(&mut store, transaction, &committee)?
                else {
                    panic!("expected only local inputs");
                };
                let PreparedTransaction::LocalOnly(local) = manager.prepare_local_input_transaction(
                    &mut store,
                    &committee,
                    transaction,
                    local_versions,
                    block,
                    pinned_to(1),
                    false,
                )?
                else {
                    panic!("expected a local-only preparation");
                };
                prepared.push(match *local {
                    LocalPreparedTransaction::Accept { execution, lock_status } => Prepared {
                        execution,
                        is_locked: !lock_status.is_hard_conflict(),
                    },
                    LocalPreparedTransaction::EarlyAbort { execution } => Prepared {
                        execution,
                        is_locked: false,
                    },
                });
            }
            let locks = store
                .new_locks()
                .iter()
                .flat_map(|(id, locks)| {
                    locks
                        .iter()
                        .map(|lock| (*lock.transaction_id(), id.clone(), lock.lock_type()))
                })
                .collect();
            Ok::<_, BlockTransactionExecutorError>((prepared, locks))
        })
        .unwrap()
    }

    fn assert_fee_intent_commit(prepared: &Prepared) {
        assert!(prepared.execution.decision().is_commit());
        assert!(prepared.is_locked);
        let (_, reason) = prepared
            .execution
            .result()
            .finalize
            .fee_accept_transaction_reject()
            .expect("a fee-intent commit");
        assert!(matches!(reason, RejectReason::FailedToLockOutputs(_)), "{reason}");
    }

    #[test]
    fn creating_an_output_that_already_exists_commits_the_fee_intent() {
        let (prepared, locks) = prepare_in_one_block(&[component_id(2)], &[LocalOnlySpec {
            signer: 1,
            vault: component_id(1),
            creates: vec![component_id(3), component_id(2)],
            with_fallback: true,
        }]);

        assert_fee_intent_commit(&prepared[0]);
        let diff = prepared[0].execution.result().finalize.any_accept().unwrap();
        assert!(
            !diff
                .up_iter()
                .any(|(id, _)| [component_id(2), component_id(3)].contains(id))
        );
        let evidence = prepared[0].execution.to_evidence(NumPreshards::P256, 1);
        assert_eq!(evidence.num_shard_groups(), 1);
        assert!(
            !evidence
                .all_outputs_iter()
                .any(|(_, id, _)| [component_id(2), component_id(3)].contains(id))
        );
        // The failed attempt's locks are released, so nothing only the main intent creates stays locked
        for id in [component_id(2), component_id(3)] {
            assert!(!locks.iter().any(|(_, locked, _)| *locked == id), "{id} in {locks:?}");
        }
        assert!(
            locks
                .iter()
                .any(|(_, id, lock)| *id == component_id(1) && lock.is_write())
        );
        assert!(
            locks
                .iter()
                .any(|(_, id, lock)| *id == component_id(1) && lock.is_output())
        );
    }

    #[test]
    fn creating_an_output_that_already_exists_without_a_fee_intent_aborts_holding_no_locks() {
        let (prepared, locks) = prepare_in_one_block(&[component_id(2)], &[LocalOnlySpec {
            signer: 1,
            vault: component_id(1),
            creates: vec![component_id(3), component_id(2)],
            with_fallback: false,
        }]);
        assert!(prepared[0].execution.decision().is_abort());
        assert!(!prepared[0].is_locked);
        assert!(locks.is_empty(), "{locks:?}");
    }

    #[test]
    fn creating_an_output_another_transaction_in_the_block_creates_commits_the_fee_intent() {
        let (prepared, locks) = prepare_in_one_block(&[], &[
            LocalOnlySpec {
                signer: 1,
                vault: component_id(1),
                creates: vec![component_id(3)],
                with_fallback: true,
            },
            LocalOnlySpec {
                signer: 2,
                vault: component_id(4),
                creates: vec![component_id(3)],
                with_fallback: true,
            },
        ]);

        assert!(prepared[0].execution.result().finalize.is_full_accept());
        assert_fee_intent_commit(&prepared[1]);
        let first = prepared[0].execution.result().finalize.transaction_hash;
        let holders = locks
            .iter()
            .filter(|(_, id, _)| *id == component_id(3))
            .map(|(tx, _, _)| tx.as_hash())
            .collect::<Vec<_>>();
        assert_eq!(holders, vec![first]);
    }

    #[test]
    fn an_aborted_transaction_does_not_hold_the_outputs_it_failed_to_create() {
        let (prepared, _) = prepare_in_one_block(&[component_id(2)], &[
            LocalOnlySpec {
                signer: 1,
                vault: component_id(1),
                creates: vec![component_id(3), component_id(2)],
                with_fallback: false,
            },
            LocalOnlySpec {
                signer: 2,
                vault: component_id(4),
                creates: vec![component_id(3)],
                with_fallback: true,
            },
        ]);

        assert!(prepared[0].execution.decision().is_abort());
        assert!(prepared[1].execution.result().finalize.is_full_accept());
        assert!(prepared[1].is_locked);
    }

    #[test]
    fn a_substate_is_taken_if_it_exists_or_another_local_only_transaction_creates_it() {
        // Shard group 0 of 2 holds the first half of the shards
        let committee = CommitteeInfo::new(
            NumPreshards::P256,
            1,
            2,
            ShardGroup::new(0u32, 127u32),
            Epoch(1),
            VotePower::of(1),
        );
        let (local, foreign): (Vec<_>, Vec<_>) = (1..=u8::MAX)
            .map(component_id)
            .partition(|id| committee.includes_substate_id(id));
        let [existing, created, fresh] = [0, 1, 2].map(|i| local[i].clone());
        let foreign = foreign[0].clone();
        let creator = TransactionId::new([1; 32]);
        let other = TransactionId::new([2; 32]);

        let dir = tempfile::tempdir().unwrap();
        let db = RocksDbStateStore::<String>::open(dir.path().join("db"), Default::default()).unwrap();
        let zero = Block::zero_block(Network::LocalNet, NumPreshards::P256);
        db.with_write_tx(|tx| zero.insert(tx)).unwrap();
        db.with_read_tx(|tx| {
            let mut store = PendingSubstateStore::init(tx, zero.as_leaf(), NumPreshards::P256)?;
            store.put(SubstateChange::Up {
                id: existing.clone(),
                shard: shard_of(&existing),
                substate: Box::new(substate(0)),
            })?;
            store.try_lock(
                creator,
                &VersionedSubstateIdLockIntent::output(versioned(&created, 0)),
                true,
            )?;

            let taken = |transaction_id: &TransactionId, id: &SubstateId| {
                LocalCreationCheck::new(&store, &committee, transaction_id)
                    .is_creation_taken(id)
                    .unwrap()
            };
            assert!(taken(&other, &existing));
            assert!(taken(&other, &created));
            assert!(!taken(&creator, &created));
            assert!(!taken(&other, &fresh));
            assert!(!taken(&other, &foreign));
            Ok::<_, SubstateStoreError>(())
        })
        .unwrap();
    }

    fn multishard_transaction(signer: u64) -> TransactionRecord {
        TransactionRecord::new(
            Transaction::builder_localnet(Epoch(100))
                .add_input(SubstateRequirement::unversioned(component_id(1)))
                .build_and_seal(&PrivateKey::from(signer)),
        )
    }

    /// Locks the local outputs of `execution` at LocalAccept in a block of a network of one shard group in which
    /// `existing` already exist. Returns the locked execution and the substates the block holds a lock on.
    fn lock_local_accept_outputs(
        existing: &[SubstateId],
        transaction: &TransactionRecord,
        execution: TransactionExecution,
        fee_intent_only: bool,
    ) -> (LockedMultishardExecution, Vec<SubstateId>) {
        let dir = tempfile::tempdir().unwrap();
        let db = RocksDbStateStore::<String>::open(dir.path().join("db"), Default::default()).unwrap();
        let zero = Block::zero_block(Network::LocalNet, NumPreshards::P256);
        db.with_write_tx(|tx| zero.insert(tx)).unwrap();
        db.with_read_tx(|tx| {
            let mut store = PendingSubstateStore::init(tx, zero.as_leaf(), NumPreshards::P256)?;
            for id in existing {
                store.put(SubstateChange::Up {
                    shard: shard_of(id),
                    id: id.clone(),
                    substate: Box::new(substate(0)),
                })?;
            }
            let locked = lock_multishard_execution(
                &mut store,
                &single_committee(Epoch(1)),
                *transaction.id(),
                execution,
                MultishardLockScope::LocalOutputs,
                fee_intent_only,
            )?;
            let locked_ids = store.new_locks().keys().cloned().collect();
            Ok::<_, BlockTransactionExecutorError>((locked, locked_ids))
        })
        .unwrap()
    }

    fn assert_locks_only_the_fee_intent(locked: &LockedMultishardExecution, locked_ids: &[SubstateId]) {
        assert!(locked.fee_intent_only);
        assert!(locked.execution.is_fee_intent_commit);
        assert!(!locked.lock_status.is_any_failed(), "{:?}", locked.lock_status);
        assert!(locked.execution.result().finalize.is_fee_only());
        assert!(locked_ids.contains(&component_id(1)), "{locked_ids:?}");
        let rejected = locked
            .execution
            .rejected_outputs()
            .iter()
            .map(|output| output.substate_id().clone())
            .collect::<Vec<_>>();
        for id in [component_id(2), component_id(3)] {
            assert!(!locked_ids.contains(&id), "{id} in {locked_ids:?}");
            assert!(rejected.contains(&id), "{id} not in {rejected:?}");
        }
        assert_eq!(rejected.len(), 2, "{rejected:?}");
    }

    #[test]
    fn a_shard_group_creating_a_multishard_output_that_exists_commits_the_fee_intent() {
        let transaction = multishard_transaction(1);
        let execution = executing_to(
            &transaction,
            &component_id(1),
            &[&component_id(3), &component_id(2)],
            true,
        );

        let (locked, locked_ids) = lock_local_accept_outputs(&[component_id(2)], &transaction, execution, false);

        assert_locks_only_the_fee_intent(&locked, &locked_ids);
    }

    #[test]
    fn a_shard_group_another_found_commits_the_fee_intent_locks_only_the_fee_intent() {
        let transaction = multishard_transaction(1);
        let execution = executing_to(
            &transaction,
            &component_id(1),
            &[&component_id(3), &component_id(2)],
            true,
        );

        let (locked, locked_ids) = lock_local_accept_outputs(&[], &transaction, execution, true);

        assert_locks_only_the_fee_intent(&locked, &locked_ids);
    }

    #[test]
    fn a_stored_fee_intent_commit_is_locked_as_committing_only_the_fee_intent() {
        let transaction = multishard_transaction(1);
        let mut execution = executing_to(
            &transaction,
            &component_id(1),
            &[&component_id(3), &component_id(2)],
            true,
        );
        let fee_intent_commit = execution
            .take_fee_intent_commit(RejectReason::FailedToLockOutputs("exists".to_string()))
            .unwrap();

        let (locked, locked_ids) = lock_local_accept_outputs(&[], &transaction, fee_intent_commit, false);

        assert_locks_only_the_fee_intent(&locked, &locked_ids);
    }

    #[test]
    fn a_multishard_output_that_exists_without_a_fee_intent_fails_to_lock() {
        let transaction = multishard_transaction(1);
        let execution = executing_to(&transaction, &component_id(1), &[&component_id(2)], false);

        let (locked, _) = lock_local_accept_outputs(&[component_id(2)], &transaction, execution, false);

        assert!(!locked.fee_intent_only);
        assert!(locked.lock_status.is_hard_conflict());
        assert!(locked.execution.result().finalize.is_full_accept());
    }

    #[test]
    fn adopting_the_fee_intent_commit_keeps_an_aborting_execution() {
        let transaction = multishard_transaction(1);
        let abort = TransactionExecution::abort(transaction.id(), RejectReason::ForeignPledgeInputConflict);

        let adopted = adopt_fee_intent_commit(transaction.id(), abort).unwrap();

        assert!(adopted.decision().is_abort());
    }

    #[test]
    fn adopting_the_fee_intent_commit_of_an_execution_without_one_is_an_invariant_error() {
        let transaction = multishard_transaction(1);
        let execution = executing_to(&transaction, &component_id(1), &[&component_id(2)], false);

        let err = adopt_fee_intent_commit(transaction.id(), execution).unwrap_err();

        assert!(matches!(err, BlockTransactionExecutorError::InvariantError(_)), "{err}");
    }

    /// A shard group that accepted the full execution holds no lock on what only the fee intent writes
    #[test]
    fn a_fee_intent_writing_what_the_full_execution_does_not_is_not_committed_alone() {
        let transaction = multishard_transaction(1);
        let mut execution = executing_to(&transaction, &component_id(1), &[&component_id(2)], true);
        execution
            .fee_intent_fallback
            .as_mut()
            .unwrap()
            .diff
            .up(component_id(4), substate(0));

        let fee_intent_commit = take_multishard_fee_intent_commit(&mut execution);

        assert!(fee_intent_commit.is_none());
    }
}
