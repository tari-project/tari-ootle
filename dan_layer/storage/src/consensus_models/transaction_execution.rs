//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, time::Duration};

use serde::{Deserialize, Serialize};
use tari_dan_common_types::{NodeHeight, NumPreshards};
use tari_engine_types::commit_result::{ExecuteResult, RejectReason};
use tari_transaction::TransactionId;
use time::PrimitiveDateTime;

use crate::{
    consensus_models::{BlockId, Decision, Evidence, LeafBlock, VersionedSubstateIdLockIntent},
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct TransactionExecution {
    pub result: ExecuteResult,
    pub resolved_inputs: Vec<VersionedSubstateIdLockIntent>,
    pub resulting_outputs: Vec<VersionedSubstateIdLockIntent>,
}

impl TransactionExecution {
    pub fn new(
        result: ExecuteResult,
        resolved_inputs: Vec<VersionedSubstateIdLockIntent>,
        resulting_outputs: Vec<VersionedSubstateIdLockIntent>,
    ) -> Self {
        Self {
            result,
            resolved_inputs,
            resulting_outputs,
        }
    }

    pub fn abort(transaction_id: &TransactionId, reject_reason: RejectReason) -> Self {
        Self {
            result: ExecuteResult::new_rejected(transaction_id.as_hash(), reject_reason),
            resolved_inputs: Vec::new(),
            resulting_outputs: Vec::new(),
        }
    }

    pub fn result(&self) -> &ExecuteResult {
        &self.result
    }

    pub fn into_result(self) -> ExecuteResult {
        self.result
    }

    pub fn decision(&self) -> Decision {
        Decision::from(&self.result.finalize.result)
    }

    pub fn transaction_fee(&self) -> u64 {
        if self.decision().is_abort() {
            return 0;
        }

        self.result
            .finalize
            .fee_receipt
            .total_fees_paid()
            .as_u64_checked()
            .expect("invariant: engine calculated negative fee")
    }

    pub fn resolved_inputs(&self) -> &[VersionedSubstateIdLockIntent] {
        &self.resolved_inputs
    }

    pub fn resulting_outputs(&self) -> &[VersionedSubstateIdLockIntent] {
        &self.resulting_outputs
    }

    pub fn abort_reason(&self) -> Option<&RejectReason> {
        self.result().finalize.result.any_reject()
    }

    pub fn execution_time(&self) -> Duration {
        self.result.execution_time
    }

    pub fn to_evidence(&self, num_preshards: NumPreshards, num_committees: u32) -> Evidence {
        let mut evidence = Evidence::from_lock_intents(
            num_preshards,
            num_committees,
            self.resolved_inputs().iter().chain(self.resulting_outputs()),
        );
        if self.decision().is_abort() {
            evidence.abort();
        }
        evidence
    }

    pub fn for_block(self, block: LeafBlock, transaction_id: TransactionId) -> BlockTransactionExecution {
        BlockTransactionExecution {
            block,
            transaction_id,
            execution: self,
        }
    }
}

impl TransactionExecution {
    pub fn get_finalized<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        transaction_id: &TransactionId,
    ) -> Result<Self, StorageError> {
        tx.finalized_transaction_execution_get(transaction_id)
    }

    pub fn get_finalized_time<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        transaction_id: &TransactionId,
    ) -> Result<PrimitiveDateTime, StorageError> {
        tx.finalized_transaction_execution_get_finalized_time(transaction_id)
    }
}

impl Display for TransactionExecution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TransactionExecution({}, decision: {}, execution_time: {:.2?})",
            self.result.finalize.result,
            self.decision(),
            self.execution_time()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockTransactionExecution {
    block: LeafBlock,
    transaction_id: TransactionId,
    execution: TransactionExecution,
}

impl BlockTransactionExecution {
    pub fn new(
        block: LeafBlock,
        transaction_id: TransactionId,
        result: ExecuteResult,
        resolved_inputs: Vec<VersionedSubstateIdLockIntent>,
        resulting_outputs: Vec<VersionedSubstateIdLockIntent>,
    ) -> Self {
        Self {
            block,
            transaction_id,
            execution: TransactionExecution::new(result, resolved_inputs, resulting_outputs),
        }
    }

    pub fn transaction_execution(&self) -> &TransactionExecution {
        &self.execution
    }

    pub fn into_transaction_execution(self) -> TransactionExecution {
        self.execution
    }

    pub fn block_id(&self) -> &BlockId {
        self.block.block_id()
    }

    pub fn block_height(&self) -> NodeHeight {
        self.block.height()
    }

    pub fn decision(&self) -> Decision {
        self.execution.decision()
    }

    pub fn transaction_id(&self) -> &TransactionId {
        &self.transaction_id
    }

    pub fn result(&self) -> &ExecuteResult {
        self.execution.result()
    }

    pub fn resolved_inputs(&self) -> &[VersionedSubstateIdLockIntent] {
        &self.execution.resolved_inputs
    }

    pub fn resulting_outputs(&self) -> &[VersionedSubstateIdLockIntent] {
        &self.execution.resulting_outputs
    }

    pub fn execution_time(&self) -> Duration {
        self.execution.result.execution_time
    }

    pub fn transaction_fee(&self) -> u64 {
        self.execution.transaction_fee()
    }
}

impl BlockTransactionExecution {
    pub fn insert_if_required<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<bool, StorageError> {
        tx.block_transaction_executions_insert_or_ignore(self)
    }

    /// Fetches any pending execution that happened before the given block until the commit block (parent of locked
    /// block)
    pub fn get_pending_for_block<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        transaction_id: &TransactionId,
        from_block: &LeafBlock,
    ) -> Result<Self, StorageError> {
        tx.block_transaction_executions_get_pending_for_block(transaction_id, from_block)
    }
}

impl Display for BlockTransactionExecution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BlockTransactionExecution({}, transaction_id: {}, decision: {}, execution_time: {:.2?})",
            self.block,
            self.transaction_id,
            self.decision(),
            self.execution_time()
        )
    }
}
