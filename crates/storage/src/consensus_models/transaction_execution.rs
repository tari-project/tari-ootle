//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt::Display, time::Duration};

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_consensus_types::{BlockId, Decision, LeafBlock};
use tari_engine_types::commit_result::{ExecuteResult, FeeIntentFallback, RejectReason};
use tari_ootle_common_types::{Epoch, LockIntent, NodeHeight, NumPreshards, VersionedSubstateId};
use tari_ootle_transaction::TransactionId;
use time::PrimitiveDateTime;

use crate::{
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{Evidence, VersionedSubstateIdLockIntent},
};

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionExecution {
    #[n(0)]
    pub result: ExecuteResult,
    #[n(1)]
    pub resolved_inputs: Vec<VersionedSubstateIdLockIntent>,
    #[n(2)]
    pub resulting_outputs: Vec<VersionedSubstateIdLockIntent>,
    /// What the transaction commits instead if its outputs cannot be created. Settled by the executing node and
    /// never sent to clients.
    #[n(3)]
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub fee_intent_fallback: Option<Box<FeeIntentFallback>>,
    /// This execution commits only the fee intent of a transaction that cannot create one of its outputs. Set by
    /// [`Self::take_fee_intent_commit`] and node-internal.
    #[n(4)]
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub is_fee_intent_commit: bool,
    /// The outputs of the full execution that a fee-intent commit or a rejected execution does not create. The
    /// transaction still involves their shard groups, which every shard group must count alike, so they are part of
    /// a multishard transaction's evidence but are never locked or written. Set by [`Self::take_fee_intent_commit`]
    /// and [`Self::into_abort`], and node-internal.
    #[n(5)]
    #[cbor(default)]
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub rejected_outputs: Vec<VersionedSubstateIdLockIntent>,
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
            fee_intent_fallback: None,
            is_fee_intent_commit: false,
            rejected_outputs: Vec::new(),
        }
    }

    pub fn with_fee_intent_fallback(mut self, fallback: Option<FeeIntentFallback>) -> Self {
        self.fee_intent_fallback = fallback.map(Box::new);
        self
    }

    /// The execution committing only this execution's fee intent, rejecting the rest for `reason`, or `None` if
    /// there is no fee intent to commit. Inputs keep the locks this execution resolved, which cover every input the
    /// fee intent touches. The outputs only this execution creates become the commit's rejected outputs.
    pub fn take_fee_intent_commit(&mut self, reason: RejectReason) -> Option<Self> {
        let fallback = self.fee_intent_fallback.take()?;
        let resulting_outputs = fallback
            .diff
            .up_iter()
            .map(|(id, substate)| {
                VersionedSubstateIdLockIntent::output(VersionedSubstateId::new(id.clone(), substate.version()))
            })
            .collect::<Vec<_>>();
        let fee_intent_outputs = resulting_outputs
            .iter()
            .map(|output| output.to_versioned_substate_id_ref())
            .collect::<HashSet<_>>();
        let rejected_outputs = self
            .resulting_outputs
            .iter()
            .filter(|output| !fee_intent_outputs.contains(&output.to_versioned_substate_id_ref()))
            .cloned()
            .collect();
        Some(Self {
            result: ExecuteResult {
                finalize: fallback.into_finalize_result(&self.result.finalize, reason),
                execution_time: self.result.execution_time,
                execute_epoch: self.result.execute_epoch,
                wasm_execution_points: self.result.wasm_execution_points,
                native_execution_points: self.result.native_execution_points,
            },
            resolved_inputs: self.resolved_inputs.clone(),
            resulting_outputs,
            fee_intent_fallback: None,
            is_fee_intent_commit: true,
            rejected_outputs,
        })
    }

    pub fn abort(transaction_id: &TransactionId, reject_reason: RejectReason) -> Self {
        Self {
            result: ExecuteResult::new_rejected(transaction_id.as_hash(), reject_reason, None),
            resolved_inputs: Vec::new(),
            resulting_outputs: Vec::new(),
            fee_intent_fallback: None,
            is_fee_intent_commit: false,
            rejected_outputs: Vec::new(),
        }
    }

    /// Rejects this execution for `reject_reason`, keeping the inputs and outputs it resolved so the aborting shard
    /// group's evidence still names every shard group the execution involved.
    pub fn into_abort(mut self, transaction_id: &TransactionId, reject_reason: RejectReason) -> Self {
        self.rejected_outputs.append(&mut self.resulting_outputs);
        Self {
            result: ExecuteResult::new_rejected(transaction_id.as_hash(), reject_reason, None),
            resolved_inputs: self.resolved_inputs,
            resulting_outputs: Vec::new(),
            fee_intent_fallback: None,
            is_fee_intent_commit: false,
            rejected_outputs: self.rejected_outputs,
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

        // The atom's transaction fee is the leader's share of what was paid; the burn share is carried
        // separately (see [Self::exhaust_burn]).
        self.result.finalize.fee_receipt.pre_burn_fees_paid()
    }

    /// The exhaust burn collected by the executor for this transaction. Deterministic execution output,
    /// carried into `LeaderFee::exhaust_burn` and validated by recompute-and-compare like the transaction fee.
    pub fn exhaust_burn(&self) -> u64 {
        if self.decision().is_abort() {
            return 0;
        }

        self.result.finalize.fee_receipt.exhaust_burn()
    }

    pub fn resolved_inputs(&self) -> &[VersionedSubstateIdLockIntent] {
        &self.resolved_inputs
    }

    pub fn resulting_outputs(&self) -> &[VersionedSubstateIdLockIntent] {
        &self.resulting_outputs
    }

    pub fn rejected_outputs(&self) -> &[VersionedSubstateIdLockIntent] {
        &self.rejected_outputs
    }

    /// The outputs whose shard groups the transaction involves: those it creates, and those a fee-intent commit or a
    /// rejected execution does not create.
    pub fn involved_outputs(&self) -> impl Iterator<Item = &VersionedSubstateIdLockIntent> {
        self.resulting_outputs.iter().chain(&self.rejected_outputs)
    }

    /// The inputs and outputs whose shard groups the transaction involves.
    pub fn involved_locks(&self) -> impl Iterator<Item = &VersionedSubstateIdLockIntent> {
        self.resolved_inputs.iter().chain(self.involved_outputs())
    }

    pub fn abort_reason(&self) -> Option<&RejectReason> {
        self.result().finalize.result.any_reject()
    }

    pub fn execution_time(&self) -> Duration {
        self.result.execution_time
    }

    /// The evidence of a local-only execution, listing the outputs it creates. A local-only execution's rejected
    /// outputs are local, so they involve no shard group beyond those listed.
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

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct BlockTransactionExecution {
    #[n(0)]
    block: LeafBlock,
    #[n(1)]
    transaction_id: TransactionId,
    #[n(2)]
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

    pub fn block_epoch(&self) -> Epoch {
        self.block.epoch()
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

    pub fn exhaust_burn(&self) -> u64 {
        self.execution.exhaust_burn()
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

#[cfg(test)]
mod tests {
    use tari_engine_types::commit_result::RejectReason;

    use super::*;

    /// Encodes a pre-upgrade `TransactionExecution` record written without field `#[n(5)]`.
    #[derive(Encode)]
    struct LegacyTransactionExecution {
        #[n(0)]
        result: ExecuteResult,
        #[n(1)]
        resolved_inputs: Vec<VersionedSubstateIdLockIntent>,
        #[n(2)]
        resulting_outputs: Vec<VersionedSubstateIdLockIntent>,
        #[n(3)]
        fee_intent_fallback: Option<Box<FeeIntentFallback>>,
        #[n(4)]
        is_fee_intent_commit: bool,
    }

    #[test]
    fn a_stored_execution_without_rejected_outputs_decodes_with_an_empty_vec() {
        let id = TransactionId::default();
        let legacy = LegacyTransactionExecution {
            result: ExecuteResult::new_rejected(id.as_hash(), RejectReason::ForeignPledgeInputConflict, None),
            resolved_inputs: Vec::new(),
            resulting_outputs: Vec::new(),
            fee_intent_fallback: None,
            is_fee_intent_commit: false,
        };
        let bytes = minicbor::to_vec(&legacy).unwrap();

        let decoded: TransactionExecution = minicbor::decode(&bytes).unwrap();

        assert!(decoded.rejected_outputs.is_empty());
    }
}
