//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_engine_types::substate::Substate;
use tari_ootle_common_types::{Epoch, SubstateRequirement, optional::IsNotFoundError};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StorageError,
    consensus_models::{LockedEpoch, TransactionExecution, TransactionPoolError},
};
use tari_ootle_transaction::Transaction;

use crate::hotstuff::substate_store::{LockFailedError, SubstateStoreError};

#[derive(thiserror::Error, Debug)]
pub enum BlockTransactionExecutorError {
    #[error("Execution thread failure: {0}")]
    ExecutionThreadFailure(String),
    #[error(transparent)]
    StorageError(#[from] StorageError),
    #[error("State store error: {0}")]
    StateStoreError(String),
    #[error("Substate store error: {0}")]
    SubstateStoreError(#[from] SubstateStoreError),
    #[error("Transaction validation error: {0}")]
    TransactionValidationError(String),
    #[error("Transaction pool error: {0}")]
    TransactionPoolError(#[from] TransactionPoolError),
    #[error("BUG: Invariant error: {0}")]
    InvariantError(String),
}
impl BlockTransactionExecutorError {
    pub fn is_substate_down_error(&self) -> bool {
        matches!(
            self,
            BlockTransactionExecutorError::SubstateStoreError(SubstateStoreError::SubstateIsDown { .. }) |
                BlockTransactionExecutorError::SubstateStoreError(SubstateStoreError::LockFailed(
                    LockFailedError::SubstateIsDown { .. }
                ))
        )
    }
}

impl IsNotFoundError for BlockTransactionExecutorError {
    fn is_not_found_error(&self) -> bool {
        match self {
            BlockTransactionExecutorError::StorageError(err) => err.is_not_found_error(),
            BlockTransactionExecutorError::SubstateStoreError(err) => err.is_not_found_error(),
            _ => false,
        }
    }
}

pub trait BlockTransactionExecutor<TStateStore: StateStore> {
    fn validate<TTx: StateStoreReadTransaction>(
        &self,
        tx: &TTx,
        current_epoch: Epoch,
        transaction: &Transaction,
    ) -> Result<(), BlockTransactionExecutorError>;

    fn execute(
        &self,
        transaction: &Transaction,
        locked_epoch: LockedEpoch,
        resolved_inputs: &HashMap<SubstateRequirement, Substate>,
    ) -> Result<TransactionExecution, BlockTransactionExecutorError>;
}
