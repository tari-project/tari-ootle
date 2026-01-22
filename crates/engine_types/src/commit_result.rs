//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{
    fmt::{self, Display, Formatter},
    time::Duration,
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tari_template_lib::{
    models::{ComponentAddress, ResourceAddress},
    types::{Hash, TemplateAddress},
};

use crate::{
    events::Event,
    fees::FeeReceipt,
    instruction_result::InstructionResult,
    logs::LogEntry,
    resource::Resource,
    substate::SubstateDiff,
    transaction_receipt::TransactionReceipt,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ExecuteResult {
    /// The finalized result to commit. If the fee transaction succeeds but the transaction fails, this will be accept.
    pub finalize: FinalizeResult,
    /// The time taken to execute the transaction (excluding finalization).
    #[cfg_attr(feature = "ts", ts(type = "{secs: number, nanos: number}"))]
    pub execution_time: Duration,
    /// The epoch during which the transaction was executed. This may be None if consensus aborted before being able to
    /// lock to an epoch.
    pub execute_epoch: Option<u64>,
}

impl ExecuteResult {
    pub fn new_rejected(transaction_hash: Hash, reason: RejectReason, execute_epoch: Option<u64>) -> Self {
        Self {
            finalize: FinalizeResult::new_rejected(transaction_hash, reason),
            execution_time: Duration::default(),
            execute_epoch,
        }
    }

    /// Returns the ExecuteResult if successful, or panic if the transaction was not successful.
    #[track_caller]
    pub fn unwrap_success(self) -> Self {
        self.expect_success();
        self
    }

    /// Returns the SubstateDiff if the transaction was successful, or panic if the transaction was not successful.
    #[track_caller]
    pub fn expect_success(&self) -> &SubstateDiff {
        let diff = self.expect_finalization_success();

        if let Some(reason) = self.finalize.any_reject() {
            panic!("Transaction failed: {}", reason);
        }

        diff
    }

    #[track_caller]
    pub fn expect_finalization_failure(&self) -> &RejectReason {
        match self.finalize.result {
            TransactionResult::Accept(_) => panic!("Expected transaction to fail but it succeeded"),
            TransactionResult::AcceptFeeRejectRest(_, ref reason) => reason,
            TransactionResult::Reject(ref reason) => reason,
        }
    }

    #[track_caller]
    pub fn expect_fee_accept_transaction_reject(&self) -> (&SubstateDiff, &RejectReason) {
        match self.finalize.result {
            TransactionResult::AcceptFeeRejectRest(ref diff, ref reason) => (diff, reason),
            _ => panic!(
                "Expected fee accept, transaction reject but got {:?}",
                self.finalize.result
            ),
        }
    }

    #[track_caller]
    pub fn expect_failure(&self) -> &RejectReason {
        if let Some(reason) = self.finalize.any_reject() {
            reason
        } else {
            panic!("Transaction succeeded but it was expected to fail");
        }
    }

    #[track_caller]
    pub fn expect_finalization_success(&self) -> &SubstateDiff {
        match self.finalize.result {
            TransactionResult::Accept(ref diff) => diff,
            TransactionResult::AcceptFeeRejectRest(ref diff, _) => diff,
            TransactionResult::Reject(ref reason) => panic!("Transaction failed: {}", reason),
        }
    }

    #[track_caller]
    pub fn expect_fees_paid_in_full(&self) -> &FeeReceipt {
        let receipt = &self.finalize.fee_receipt;
        assert!(receipt.is_paid_in_full(), "Fees not paid in full");
        receipt
    }

    #[track_caller]
    pub fn expect_return<T: DeserializeOwned>(&self, index: usize) -> T {
        self.finalize
            .execution_results
            .get(index)
            .expect("No return value at index")
            .decode()
            .expect("Failed to decode return value")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FinalizeResult {
    pub transaction_hash: Hash,
    pub events: Vec<Event>,
    pub logs: Vec<LogEntry>,
    pub execution_results: Vec<InstructionResult>,
    pub result: TransactionResult,
    pub fee_receipt: FeeReceipt,
}

impl FinalizeResult {
    pub fn new(
        transaction_hash: Hash,
        logs: Vec<LogEntry>,
        events: Vec<Event>,
        result: TransactionResult,
        fee_receipt: FeeReceipt,
    ) -> Self {
        Self {
            transaction_hash,
            logs,
            events,
            execution_results: Vec::new(),
            result,
            fee_receipt,
        }
    }

    pub fn new_rejected(transaction_hash: Hash, reason: RejectReason) -> Self {
        Self {
            transaction_hash,
            logs: vec![],
            events: vec![],
            execution_results: Vec::new(),
            result: TransactionResult::Reject(reason),
            fee_receipt: FeeReceipt::default(),
        }
    }

    /// Returns the substate diff if the fee and main intents were accepted, otherwise None.
    pub fn accept(&self) -> Option<&SubstateDiff> {
        self.result.accept()
    }

    /// Returns the accept diff if the transaction was accepted, otherwise None.
    /// Acceptance includes fee-only acceptance.
    pub fn any_accept(&self) -> Option<&SubstateDiff> {
        self.result.any_accept()
    }

    pub fn into_any_accept(self) -> Option<SubstateDiff> {
        match self.result {
            TransactionResult::Accept(diff) => Some(diff),
            TransactionResult::AcceptFeeRejectRest(diff, _) => Some(diff),
            TransactionResult::Reject(_) => None,
        }
    }

    pub fn fee_reject(&self) -> Option<&RejectReason> {
        self.result.fee_reject()
    }

    pub fn any_reject(&self) -> Option<&RejectReason> {
        self.result.any_reject()
    }

    pub fn reject(&self) -> Option<&RejectReason> {
        self.result.reject()
    }

    pub fn fee_accept_transaction_reject(&self) -> Option<(&SubstateDiff, &RejectReason)> {
        self.result.fee_accept_transaction_reject()
    }

    pub fn is_full_accept(&self) -> bool {
        matches!(self.result, TransactionResult::Accept(_))
    }

    pub fn is_accept(&self) -> bool {
        self.is_full_accept() || self.is_fee_only()
    }

    pub fn is_fee_only(&self) -> bool {
        matches!(self.result, TransactionResult::AcceptFeeRejectRest(_, _))
    }

    pub fn is_reject(&self) -> bool {
        matches!(self.result, TransactionResult::Reject(_))
    }

    pub fn get_transaction_receipt(&self) -> Option<&TransactionReceipt> {
        self.result.any_accept().and_then(|diff| {
            diff.up_iter()
                .find_map(|(_, s)| s.substate_value().as_transaction_receipt())
        })
    }

    pub fn get_components_by_template(&self, template_address: &TemplateAddress) -> Vec<ComponentAddress> {
        let mut components = Vec::new();
        if let Some(diff) = self.any_accept() {
            for (id, substate) in diff.up_iter() {
                if let Some(component) = substate.substate_value().as_component() {
                    if component.template_address == *template_address {
                        components.push(
                            id.as_component_address()
                                .expect("Substate value is a component but address is not a component address"),
                        );
                    }
                }
            }
        }
        components
    }

    pub fn created_resources(&self) -> impl Iterator<Item = (ResourceAddress, &Resource)> + '_ {
        self.any_accept()
            .into_iter()
            .flat_map(|diff| diff.up_iter())
            .filter_map(|(id, s)| {
                s.substate_value()
                    .as_resource()
                    .and_then(|r| id.as_resource_address().map(|a| (a, r)))
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum TransactionResult {
    Accept(SubstateDiff),
    AcceptFeeRejectRest(SubstateDiff, RejectReason),
    Reject(RejectReason),
}

impl TransactionResult {
    pub fn is_any_accept(&self) -> bool {
        matches!(self, Self::Accept(_) | Self::AcceptFeeRejectRest(_, _))
    }

    pub fn is_accept(&self) -> bool {
        matches!(self, Self::Accept(_))
    }

    pub fn is_reject(&self) -> bool {
        matches!(self, Self::Reject(_))
    }

    pub fn accept(&self) -> Option<&SubstateDiff> {
        match self {
            Self::Accept(substate_diff) => Some(substate_diff),
            _ => None,
        }
    }

    pub fn any_accept(&self) -> Option<&SubstateDiff> {
        match self {
            Self::Accept(substate_diff) => Some(substate_diff),
            Self::AcceptFeeRejectRest(substate_diff, _) => Some(substate_diff),
            Self::Reject(_) => None,
        }
    }

    pub fn into_any_accept(self) -> Option<SubstateDiff> {
        match self {
            Self::Accept(substate_diff) => Some(substate_diff),
            Self::AcceptFeeRejectRest(substate_diff, _) => Some(substate_diff),
            Self::Reject(_) => None,
        }
    }

    pub fn fee_reject(&self) -> Option<&RejectReason> {
        match self {
            Self::Accept(_) => None,
            Self::AcceptFeeRejectRest(_, _) => None,
            Self::Reject(reject_result) => Some(reject_result),
        }
    }

    pub fn fee_accept_transaction_reject(&self) -> Option<(&SubstateDiff, &RejectReason)> {
        match self {
            Self::AcceptFeeRejectRest(substate_diff, reject_result) => Some((substate_diff, reject_result)),
            _ => None,
        }
    }

    pub fn any_reject(&self) -> Option<&RejectReason> {
        match self {
            Self::Accept(_) => None,
            Self::AcceptFeeRejectRest(_, reject_result) => Some(reject_result),
            Self::Reject(reject_result) => Some(reject_result),
        }
    }

    pub fn reject(&self) -> Option<&RejectReason> {
        match self {
            Self::Reject(reject_result) => Some(reject_result),
            _ => None,
        }
    }

    pub fn expect(self, msg: &str) -> SubstateDiff {
        match self {
            Self::Accept(substate_diff) => substate_diff,
            Self::AcceptFeeRejectRest(substate_diff, _) => substate_diff,
            Self::Reject(reject_result) => {
                panic!("{}: {:?}", msg, reject_result);
            },
        }
    }
}

impl Display for TransactionResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accept(diff) => write!(f, "Accept({} up, {} down)", diff.up_len(), diff.down_len()),
            Self::AcceptFeeRejectRest(diff, reason) => write!(
                f,
                "AcceptFee({} up, {} down), Reject {}",
                diff.up_len(),
                diff.down_len(),
                reason
            ),
            Self::Reject(reason) => write!(f, "Reject: {}", reason),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum RejectReason {
    ExecutionFailure(String),
    SubstateNotFound(String),
    FailedToLockInputs(String),
    FailedToLockOutputs(String),
    ForeignPledgeInputConflict,
    ForeignShardGroupDecidedToAbort {
        start_shard: u32,
        end_shard: u32,
        abort_reason: AbortReason,
    },
    InsufficientFeesPaid(String),
    FeePaymentInMainIntent,
    Abort {
        reason: AbortReason,
    },
}

impl Display for RejectReason {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::ExecutionFailure(msg) => write!(f, "Execution failure: {}", msg),
            Self::SubstateNotFound(msg) => write!(f, "Substates not found: {}", msg),
            Self::FailedToLockInputs(msg) => write!(f, "Failed to lock inputs: {}", msg),
            Self::FailedToLockOutputs(msg) => write!(f, "Failed to lock outputs: {}", msg),
            Self::ForeignPledgeInputConflict => {
                write!(f, "Transaction conflicts with an existing foreign pledge",)
            },
            Self::ForeignShardGroupDecidedToAbort {
                start_shard,
                end_shard,
                abort_reason,
            } => {
                write!(
                    f,
                    "Foreign shard group ({start_shard}-{end_shard}) decided to abort: {abort_reason}"
                )
            },
            Self::InsufficientFeesPaid(msg) => write!(f, "Insufficient fees paid: {}", msg),
            Self::FeePaymentInMainIntent => {
                write!(f, "Fee payment found in main intent instead of fee intent")
            },
            Self::Abort { reason } => write!(f, "Abnormal abort: {reason}"),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum AbortReason {
    ForeignPledgeInputConflict,
    LockInputsFailed,
    LockOutputsFailed,
    LockInputsOutputsFailed,
    ExecutionFailure,
    OneOrMoreInputsNotFound,
    InsufficientFeesPaid,
    FeePaymentInMainIntent,
}

impl Display for AbortReason {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<&RejectReason> for AbortReason {
    fn from(reject_reason: &RejectReason) -> Self {
        match reject_reason {
            RejectReason::Abort { reason } => *reason,
            RejectReason::ExecutionFailure(_) => Self::ExecutionFailure,
            RejectReason::SubstateNotFound(_) => Self::OneOrMoreInputsNotFound,
            RejectReason::ForeignPledgeInputConflict => Self::ForeignPledgeInputConflict,
            RejectReason::FailedToLockInputs(_) => Self::LockInputsFailed,
            RejectReason::FailedToLockOutputs(_) => Self::LockOutputsFailed,
            RejectReason::ForeignShardGroupDecidedToAbort { abort_reason, .. } => *abort_reason,
            RejectReason::InsufficientFeesPaid(_) => Self::InsufficientFeesPaid,
            RejectReason::FeePaymentInMainIntent => Self::FeePaymentInMainIntent,
        }
    }
}
