//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    iter,
    sync::{Arc, Mutex},
};

use tari_consensus::traits::{BlockTransactionExecutor, BlockTransactionExecutorError};
use tari_engine::state_store::{memory::MemoryStateStore, new_memory_store, StateWriter};
use tari_engine_types::{
    substate::{Substate, SubstateId},
    transaction_receipt::TransactionReceiptAddress,
    virtual_substate::{VirtualSubstate, VirtualSubstateId, VirtualSubstates},
};
use tari_ootle_common_types::{
    displayable::Displayable,
    Epoch,
    LockIntent,
    SubstateLockType,
    SubstateRequirement,
    VersionedSubstateId,
};
use tari_ootle_storage::{
    consensus_models::{TransactionExecution, VersionedSubstateIdLockIntent},
    StateStore,
};
use tari_ootle_transaction::{Transaction, TransactionId};
use tari_template_lib::prelude::XTR;

use crate::support::{create_execution_result_for_transaction, executions_store::TestExecutionSpecStore, TestAddress};

#[derive(Debug, Clone)]
pub struct TestBlockTransactionProcessor {
    address: TestAddress,
    store: TestExecutionSpecStore,
    history: Arc<Mutex<HashMap<TransactionId, TestHistoricalExecution>>>,
}

impl TestBlockTransactionProcessor {
    pub fn new(address: TestAddress, store: TestExecutionSpecStore) -> Self {
        Self {
            address,
            store,
            history: Default::default(),
        }
    }

    pub fn store(&self) -> &TestExecutionSpecStore {
        &self.store
    }

    pub fn get_history(&self) -> HashMap<TransactionId, TestHistoricalExecution> {
        self.history.lock().unwrap().clone()
    }

    fn add_substates_to_memory_db<'a, I: IntoIterator<Item = (&'a SubstateRequirement, &'a Substate)>>(
        inputs: I,
        out: &mut MemoryStateStore,
    ) -> Result<(), BlockTransactionExecutorError> {
        // TODO: pass the impl SubstateStore directly into the engine
        for (id, substate) in inputs {
            out.set_state(id.substate_id().clone(), substate.clone())
                .map_err(|e| BlockTransactionExecutorError::StateStoreError(e.to_string()))?;
        }

        Ok(())
    }
}

impl<TStateStore: StateStore> BlockTransactionExecutor<TStateStore> for TestBlockTransactionProcessor {
    fn validate(
        &self,
        _tx: &TStateStore::ReadTransaction<'_>,
        _current_epoch: Epoch,
        _transaction: &Transaction,
    ) -> Result<(), BlockTransactionExecutorError> {
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn execute(
        &self,
        transaction: &Transaction,
        current_epoch: Epoch,
        resolved_inputs: &HashMap<SubstateRequirement, Substate>,
    ) -> Result<TransactionExecution, BlockTransactionExecutorError> {
        let id = transaction.calculate_id();

        log::info!(
            "[{}] Transaction {} executing. {} input(s)",
            self.address,
            id,
            resolved_inputs.len()
        );

        // Create a memory db with all the input substates, needed for the transaction execution
        let mut state_db = new_memory_store();
        Self::add_substates_to_memory_db(resolved_inputs, &mut state_db)?;

        let mut virtual_substates = VirtualSubstates::new();
        virtual_substates.insert(
            VirtualSubstateId::CurrentEpoch,
            VirtualSubstate::CurrentEpoch(current_epoch.as_u64()),
        );

        let spec = self.store.get(&id).unwrap_or_else(|| {
            panic!(
                "[{}] Missing execution spec for transaction {}",
                self.address,
                transaction.calculate_id()
            )
        });

        let resolved_inputs = spec
            .input_locks
            .into_iter()
            // Implicitly add XTR as read lock for all transactions
            .chain(iter::once((SubstateId::from(XTR), SubstateLockType::Read)))
            .map(|(substate_id, lock_type)| {
                let substate = resolved_inputs.get(&substate_id).unwrap_or_else(|| {
                    panic!(
                        "[{}] Consensus did not provide input substate {} for transaction {}. In spec: {}. Provided: \
                         {}",
                        self.address,
                        substate_id,
                        id,
                        lock_type,
                        resolved_inputs.keys().collect::<Vec<_>>().display()
                    )
                });
                VersionedSubstateIdLockIntent::new(
                    VersionedSubstateId::new(substate_id, substate.version()),
                    lock_type,
                    true,
                )
            })
            .collect::<Vec<_>>();

        let resulting_outputs = spec
            .new_outputs
            .into_iter()
            .map(|substate_id| VersionedSubstateId::new(substate_id, 0))
            // Generate corresponding up substates to all consumed inputs
            .chain(
                resolved_inputs.iter().filter(|input| input.lock_type().is_write())
                    .map(|input| input.versioned_substate_id().to_next_version()),
            )
            .chain(iter::once(VersionedSubstateId::new(
                SubstateId::TransactionReceipt(TransactionReceiptAddress::from(transaction.calculate_id())),
                0,
            )))
            .map(VersionedSubstateIdLockIntent::output)
            .chain(
                spec.validator_fee_withdrawals
                    .iter()
                    .filter_map(|w| {
                        let input = resolved_inputs.iter().find(|i| i.versioned_substate_id().substate_id().as_validator_fee_pool_address() == Some(w.address))?;
                        Some(VersionedSubstateIdLockIntent::output(VersionedSubstateId::new(w.address, input.version() + 1)))
                    })
            )
            .collect::<Vec<_>>();

        let result = create_execution_result_for_transaction(
            transaction,
            spec.decision,
            spec.fee,
            &resolved_inputs,
            &resulting_outputs,
            spec.validator_fee_withdrawals,
        );

        let resulting_outputs = result
            .finalize
            .result
            .any_accept()
            .map(|diff| {
                diff.up_iter()
                    .map(|(addr, substate)| {
                        VersionedSubstateIdLockIntent::output(VersionedSubstateId::new(
                            addr.clone(),
                            substate.version(),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let executed = TransactionExecution::new(result, resolved_inputs, resulting_outputs);
        log::info!(
            "[{}] Transaction {} executed in {:.2?}. {}",
            self.address,
            id,
            executed.execution_time(),
            executed.result().finalize.result
        );
        self.history.lock().unwrap().insert(id, TestHistoricalExecution {
            execution: executed.clone(),
            current_epoch,
        });
        Ok(executed)
    }
}

#[derive(Debug, Clone)]
pub struct TestHistoricalExecution {
    pub execution: TransactionExecution,
    pub current_epoch: Epoch,
}
