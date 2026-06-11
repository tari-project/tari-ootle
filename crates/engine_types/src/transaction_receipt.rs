//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display, str::FromStr};

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

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionReceipt {
    #[n(0)]
    pub outcome: FinalizeOutcome,
    #[n(1)]
    pub diff_summary: DiffSummary,
    #[n(2)]
    #[cbor(with = "tari_bor::adapters::boxed_slice")]
    pub fee_withdrawals: Box<[ValidatorFeeWithdrawal]>,
    #[n(3)]
    #[cbor(with = "tari_bor::adapters::boxed_slice")]
    pub events: Box<[Event]>,
    #[n(4)]
    #[cbor(with = "tari_bor::adapters::boxed_slice")]
    pub logs: Box<[LogEntry]>,
    #[n(5)]
    pub fee_receipt: FeeReceipt,
    #[n(6)]
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

#[derive(
    Debug,
    Clone,
    Copy,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum FinalizeOutcome {
    #[n(0)]
    Commit,
    #[n(1)]
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

impl Display for FinalizeOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Commit => write!(f, "Commit"),
            Self::FeeIntentCommit => write!(f, "FeeIntentCommit"),
        }
    }
}

impl FromStr for FinalizeOutcome {
    type Err = FinalizeOutcomeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Commit" => Ok(Self::Commit),
            "FeeIntentCommit" => Ok(Self::FeeIntentCommit),
            _ => Err(FinalizeOutcomeParseError),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid FinalizeOutcome string")]
pub struct FinalizeOutcomeParseError;

#[derive(
    Debug,
    Clone,
    Default,
    minicbor::Encode,
    minicbor::Decode,
    minicbor::CborLen,
    Serialize,
    Deserialize,
    borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiffSummary {
    #[n(0)]
    #[cbor(with = "tari_bor::adapters::boxed_slice")]
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

#[derive(
    Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen, Serialize, Deserialize, borsh::BorshSerialize,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpSubstate {
    #[n(0)]
    pub substate_id: SubstateId,
    #[n(1)]
    pub version: u32,
    #[n(2)]
    pub value_hash: Hash32,
}
