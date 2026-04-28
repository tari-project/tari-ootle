//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_lib::types::Hash32;

use crate::{
    Epoch,
    ValidatorFeeWithdrawal,
    events::Event,
    fees::FeeReceipt,
    logs::LogEntry,
    substate::{SubstateDiff, SubstateId, hash_substate},
};

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionReceipt {
    pub outcome: FinalizeOutcome,
    pub diff_summary: DiffSummary,
    pub fee_withdrawals: Box<[ValidatorFeeWithdrawal]>,
    pub events: Box<[Event]>,
    pub logs: Box<[LogEntry]>,
    pub fee_receipt: FeeReceipt,
    pub epoch: Epoch,
}

impl TransactionReceipt {
    pub fn outcome(&self) -> &FinalizeOutcome {
        &self.outcome
    }

    pub fn diff_summary(&self) -> &DiffSummary {
        &self.diff_summary
    }

    pub fn fee_withdrawals(&self) -> &[ValidatorFeeWithdrawal] {
        &self.fee_withdrawals
    }

    pub fn logs(&self) -> &[LogEntry] {
        &self.logs
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn fee_receipt(&self) -> &FeeReceipt {
        &self.fee_receipt
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum FinalizeOutcome {
    Commit,
    FeeIntentCommit,
}

impl FinalizeOutcome {
    pub fn is_commit(&self) -> bool {
        matches!(self, FinalizeOutcome::Commit)
    }

    pub fn is_fee_intent_commit(&self) -> bool {
        matches!(self, FinalizeOutcome::FeeIntentCommit)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiffSummary {
    pub upped: Box<[UpSubstate]>,
}

impl DiffSummary {
    pub fn from_diff(diff: &SubstateDiff, epoch: Epoch) -> Self {
        Self {
            upped: diff
                .up_iter()
                .map(|(id, s)| UpSubstate {
                    substate_id: id.clone(),
                    version: s.version(),
                    value_hash: hash_substate(s.substate_value(), s.version(), epoch),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpSubstate {
    pub substate_id: SubstateId,
    pub version: u32,
    pub value_hash: Hash32,
}
