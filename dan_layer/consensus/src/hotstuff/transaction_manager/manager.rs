//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, iter, marker::PhantomData};

use indexmap::{IndexMap, IndexSet};
use log::*;
use tari_dan_common_types::{
    committee::CommitteeInfo,
    optional::{IsNotFoundError, Optional},
    Epoch,
    LockIntent,
    SubstateRequirement,
    SubstateRequirementRef,
    ToSubstateAddress,
    VersionedSubstateId,
};
use tari_dan_storage::{
    consensus_models::{
        BlockId,
        BlockTransactionExecution,
        Decision,
        Evidence,
        ExecutedTransaction,
        RequireLockIntentRef,
        SubstateRequirementLockIntent,
        TransactionExecution,
        TransactionPoolRecord,
    },
    StateStore,
};
use tari_engine_types::{
    commit_result::RejectReason,
    substate::Substate,
    transaction_receipt::TransactionReceiptAddress,
};
use tari_transaction::{Transaction, TransactionId};

use super::{PledgedTransaction, PreparedTransaction};
use crate::{
    hotstuff::substate_store::{LockStatus, PendingSubstateStore, SubstateStoreError},
    tracing::TraceTimer,
    traits::{BlockTransactionExecutor, BlockTransactionExecutorError},
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::block_transaction_executor";

#[derive(Debug, Clone)]
pub struct ConsensusTransactionManager<TExecutor, TStateStore> {
    executor: TExecutor,
    _store: PhantomData<TStateStore>,
}

impl<TStateStore: StateStore, TExecutor: BlockTransactionExecutor<TStateStore>>
    ConsensusTransactionManager<TExecutor, TStateStore>
{
    pub fn new(executor: TExecutor) -> Self {
        Self {
            executor,
            _store: PhantomData,
        }
    }

    fn resolve_local_versions<'a>(
        &self,
        store: &PendingSubstateStore<TStateStore>,
        local_committee_info: &CommitteeInfo,
        transaction: &'a Transaction,
    ) -> Result<(IndexMap<SubstateRequirement, u32>, IndexSet<SubstateRequirementRef<'a>>), BlockTransactionExecutorError>
    {
        let mut resolved_substates = IndexMap::with_capacity(transaction.num_unique_inputs());

        let mut non_local_inputs = IndexSet::new();
        for input in transaction.all_inputs_iter() {
            if !local_committee_info.includes_substate_id(input.substate_id()) {
                non_local_inputs.insert(input);
                continue;
            }

            match input.version() {
                Some(version) => {
                    let id = input.with_version(version).to_owned();
                    store.lock_assert_is_up(&id)?;
                    info!(target: LOG_TARGET, "Resolved LOCAL substate: {id}");
                    resolved_substates.insert(id.into(), version);
                },
                None => {
                    let latest = store.get_latest_version(input.substate_id())?;
                    if latest.is_down() {
                        return Err(SubstateStoreError::SubstateIsDown {
                            id: input.with_version(latest.version()).to_owned(),
                        }
                        .into());
                    }
                    info!(target: LOG_TARGET, "Resolved LOCAL unversioned substate: {input} to version {}",latest.version());
                    resolved_substates.insert(input.to_owned(), latest.version());
                },
            }
        }
        Ok((resolved_substates, non_local_inputs))
    }

    pub fn execute(
        &self,
        current_epoch: Epoch,
        pledged_transaction: PledgedTransaction,
    ) -> Result<ExecutedTransaction, BlockTransactionExecutorError> {
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
        let executed = self.executor.execute(
            pledged_transaction.transaction.into_transaction(),
            current_epoch,
            &resolved_inputs,
        )?;

        Ok(executed)
    }

    pub fn execute_or_fetch(
        &self,
        store: &mut PendingSubstateStore<TStateStore>,
        transaction: Transaction,
        current_epoch: Epoch,
        resolved_inputs: &HashMap<SubstateRequirement, Substate>,
        block_id: &BlockId,
    ) -> Result<TransactionExecution, BlockTransactionExecutorError> {
        info!(
            target: LOG_TARGET,
            "👨‍🔧 PREPARE: Executing transaction {}",
            transaction.id(),
        );
        // Might have been executed already in on propose
        if let Some(execution) = self.fetch_execution(store, transaction.id(), block_id)? {
            return Ok(execution);
        }

        let executed = self.executor.execute(transaction, current_epoch, resolved_inputs)?;

        Ok(executed.into_execution())
    }

    pub fn fetch_execution(
        &self,
        store: &PendingSubstateStore<TStateStore>,
        transaction_id: &TransactionId,
        block_id: &BlockId,
    ) -> Result<Option<TransactionExecution>, BlockTransactionExecutorError> {
        if let Some(execution) =
            BlockTransactionExecution::get_pending_for_block(store.read_transaction(), transaction_id, block_id)
                .optional()?
        {
            return Ok(Some(execution.into_transaction_execution()));
        }
        Ok(None)
    }

    #[allow(clippy::too_many_lines)]
    pub fn prepare(
        &self,
        store: &mut PendingSubstateStore<TStateStore>,
        local_committee_info: &CommitteeInfo,
        current_epoch: Epoch,
        tx_rec: &TransactionPoolRecord,
        block_id: &BlockId,
    ) -> Result<PreparedTransaction, BlockTransactionExecutorError> {
        let _timer = TraceTimer::info(LOG_TARGET, "prepare");
        let mut transaction = tx_rec.get_transaction(store.read_transaction())?;
        let transaction_id = *transaction.id();
        let mut outputs = IndexSet::new();
        outputs.insert(VersionedSubstateId::new(
            TransactionReceiptAddress::from(transaction_id),
            0,
        ));

        let (local_versions, non_local_inputs) = match self.resolve_local_versions(
            store,
            local_committee_info,
            transaction.transaction(),
        ) {
            Ok(inputs) => inputs,
            Err(err) => {
                warn!(target: LOG_TARGET, "⚠️ PREPARE: failed to resolve local inputs: {err}");
                // We only expect not found or down errors here. If we get any other error, this is fatal.
                if !err.is_not_found_error() && !err.is_substate_down_error() {
                    return Err(err);
                }
                let is_local_only = local_committee_info.includes_all_substate_addresses(
                    transaction
                        .transaction
                        .all_inputs_iter()
                        .map(|i| i.or_zero_version().to_substate_address()),
                );
                // Currently this message will differ depending on which involved shard is asked.
                // e.g. local nodes will say "failed to lock inputs", foreign nodes will say "foreign shard abort"
                transaction.abort(RejectReason::OneOrMoreInputsNotFound(err.to_string()));
                if is_local_only {
                    warn!(target: LOG_TARGET, "⚠️ PREPARE: transaction {} only contains local inputs. Will abort locally", transaction_id);
                    return Ok(PreparedTransaction::new_local_early_abort(
                        transaction
                            .into_execution()
                            .expect("invariant: abort reason is set but into_execution is None"),
                    ));
                } else {
                    warn!(target: LOG_TARGET, "⚠️ PREPARE: transaction {} has foreign inputs. Will prepare ABORT", transaction_id);
                    return Ok(PreparedTransaction::new_multishard_executed(
                        transaction
                            .into_execution()
                            .expect("invariant: abort reason is set but into_execution is None"),
                        LockStatus::default(),
                    ));
                }
            },
        };

        if local_versions.is_empty() && non_local_inputs.is_empty() {
            // Mempool validations should have not sent the transaction to consensus
            warn!(target: LOG_TARGET, "⚠️ PREPARE: NEVER HAPPEN transaction {transaction_id} has no inputs.");
            return Err(BlockTransactionExecutorError::InvariantError(format!(
                "Transaction {transaction_id} has no inputs"
            )));
        }

        // TODO: also account for the transaction receipt
        if non_local_inputs.is_empty() &&
            (local_committee_info.num_committees() == 1 || !transaction.transaction.is_global())
        {
            // CASE: All inputs are local and we can execute the transaction.
            //       Outputs may or may not be local
            if let Some(reason) = tx_rec.current_decision().abort_reason() {
                // CASE: All outputs are local, and we're aborting, so this is a local-only transaction since no
                // outputs need to be created (TODO: this should be the case, but we also count the tx receipt as
                // involvement)
                warn!(target: LOG_TARGET, "⚠️ PREPARE: Transaction {transaction_id} is ABORTED before prepare: {reason}");
                let execution = transaction.into_execution().expect("prepare: Abort without execution");
                return Ok(PreparedTransaction::new_multishard_executed(
                    execution,
                    LockStatus::default(),
                ));
            }

            let local_inputs = store.get_many(local_versions.iter().map(|(req, v)| (req.clone(), *v)))?;
            let (transaction, maybe_execution) = transaction.into_transaction_and_execution();
            let mut execution = maybe_execution
                .map(Ok)
                .unwrap_or_else(|| self.execute_or_fetch(store, transaction, current_epoch, &local_inputs, block_id))?;

            // local-only transaction can be determined if we've executed the transaction
            let is_local_only = local_committee_info
                .includes_all_substate_addresses(execution.resulting_outputs().iter().map(|o| o.to_substate_address()));
            if is_local_only {
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

                let requested_locks = execution.resolved_inputs().iter().chain(execution.resulting_outputs());
                let lock_status = store.try_lock_all(transaction_id, requested_locks, true)?;
                if let Some(err) = lock_status.hard_conflict() {
                    warn!(target: LOG_TARGET, "⚠️ PREPARE: Hard conflict when locking inputs: {err}");
                    execution.set_abort_reason(RejectReason::FailedToLockInputs(err.to_string()));
                }
                Ok(PreparedTransaction::new_local_accept(execution, lock_status))
            } else {
                info!(target: LOG_TARGET, "👨‍🔧 PREPARE: transaction {} has local inputs and foreign outputs (Local decision: {})", execution.id(), execution.decision());
                match execution.decision() {
                    Decision::Commit => {
                        // CASE: Multishard transaction, all inputs are local, consensus with output shard groups
                        // pending
                        let requested_locks = execution.resolved_inputs();
                        let lock_status = store.try_lock_all(transaction_id, requested_locks, false)?;
                        if let Some(err) = lock_status.hard_conflict() {
                            warn!(target: LOG_TARGET, "⚠️ PREPARE: Hard conflict when locking inputs: {err}");
                            execution.set_abort_reason(RejectReason::FailedToLockInputs(err.to_string()));
                        }
                        // We're committing, and one or more of the outputs are foreign
                        Ok(PreparedTransaction::new_multishard_executed(execution, lock_status))
                    },
                    Decision::Abort(reason) => {
                        // CASE: Multishard transaction, but all inputs are local, and we're aborting
                        // All outputs are local, and we're aborting, so this is a local-only transaction since no
                        // outputs need to be created
                        warn!(target: LOG_TARGET, "⚠️ PREPARE: Aborted: {reason:?}");
                        Ok(PreparedTransaction::new_local_early_abort(execution))
                    },
                }
            }
        } else {
            // // Multishard involving cross-shard inputs
            let execution = if local_versions.is_empty() {
                // We're output-only
                let foreign_pledges = transaction.get_foreign_pledges(store.read_transaction())?;

                let resolved_inputs = foreign_pledges
                    .into_iter()
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
                let execution = self.execute_or_fetch(
                    store,
                    transaction.transaction().clone(),
                    current_epoch,
                    &resolved_inputs,
                    block_id,
                )?;
                info!(
                    target: LOG_TARGET,
                    "👨‍🔧 PREPARE: Output-Only Executed transaction {} with {} decision",
                    transaction_id,
                    execution.decision()
                );

                Some(execution)
            } else {
                self.fetch_execution(store, &transaction_id, block_id)?
            };

            match execution {
                Some(execution) => {
                    if let Some(reason) = execution.decision().abort_reason() {
                        warn!(target: LOG_TARGET, "⚠️ PREPARE: Transaction {transaction_id} is ABORTED before prepare: {reason}");
                        // No locks
                        let lock_status = LockStatus::default();
                        Ok(PreparedTransaction::new_multishard_executed(execution, lock_status))
                    } else {
                        let requested_locks = execution
                            .resolved_inputs()
                            .iter()
                            .chain(execution.resulting_outputs())
                            .filter(|o| {
                                o.substate_id().is_transaction_receipt() ||
                                    local_committee_info.includes_substate_id(o.substate_id())
                            });

                        let lock_status = store.try_lock_all(transaction_id, requested_locks, false)?;
                        if let Some(err) = lock_status.hard_conflict() {
                            warn!(target: LOG_TARGET, "⚠️ PREPARE: Hard conflict when locking inputs: {err}");
                            transaction.abort(RejectReason::FailedToLockInputs(err.to_string()));
                            let execution = transaction
                                .into_execution()
                                .expect("invariant: abort reason is set but into_execution is None");
                            Ok(PreparedTransaction::new_multishard_executed(execution, lock_status))
                        } else {
                            // CASE: Multishard transaction, not executed
                            Ok(PreparedTransaction::new_multishard_executed(execution, lock_status))
                        }
                    }
                },
                None => {
                    // TODO: We do not know if the inputs locks required are Read/Write. Either we allow the user to
                    //       specify this or we can correct the locks after execution. Currently, this limitation
                    //       prevents concurrent multi-shard read locks.
                    let requested_locks = local_versions.iter().map(|(substate_id, version)| {
                        // TODO: we assume all resources are not being written to. How can we do this in the vast
                        // majority of cases but still allow (presumably rare) Access Rule updates?
                        if substate_id.substate_id().is_read_only() || substate_id.substate_id().is_resource() {
                            SubstateRequirementLockIntent::read(substate_id.clone(), *version)
                        } else {
                            SubstateRequirementLockIntent::write(substate_id.clone(), *version)
                        }
                    });

                    let mut evidence = Evidence::from_inputs_and_outputs(
                        local_committee_info.num_preshards(),
                        local_committee_info.num_committees(),
                        requested_locks.clone(),
                        iter::empty::<RequireLockIntentRef<'_>>(),
                    );
                    // Add unpledged foreign input evidence
                    for input in non_local_inputs {
                        evidence.insert_unpledged_from_substate_id(
                            local_committee_info.num_preshards(),
                            local_committee_info.num_committees(),
                            input.substate_id().clone(),
                        );
                    }
                    let lock_status = store.try_lock_all(transaction_id, requested_locks, false)?;
                    info!(
                        target: LOG_TARGET,
                        "👨‍🔧 PREPARE: Multishard transaction {transaction_id} requires additional input pledges. Partial evidence: {evidence}",
                    );
                    Ok(PreparedTransaction::new_multishard_evidence(
                        transaction_id,
                        evidence,
                        lock_status,
                    ))
                },
            }
        }
    }
}
