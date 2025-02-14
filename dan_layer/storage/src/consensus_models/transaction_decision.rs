//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    fmt::{Display, Formatter},
    str::FromStr,
};

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};
use strum::ParseError;
use strum_macros::{AsRefStr, EnumString};
use tari_engine_types::commit_result::{RejectReason, TransactionResult};
#[cfg(feature = "ts")]
use ts_rs::TS;

#[derive(Debug, Clone, thiserror::Error)]
pub enum FromStrConversionError {
    #[error("Invalid Decision string '{0}'")]
    InvalidDecision(String),
    #[error("Invalid Abort reason string '{0}': {1}")]
    InvalidAbortReason(String, ParseError),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize, BorshSerialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, export_to = "../../bindings/src/types/"))]
pub enum Decision {
    /// Decision to COMMIT the transaction
    Commit,
    /// Decision to ABORT the transaction
    Abort(AbortReason),
}

impl Decision {
    pub fn is_same_outcome(&self, other: Decision) -> bool {
        matches!(
            (self, other),
            (Decision::Commit, Decision::Commit) | (Decision::Abort(_), Decision::Abort(_))
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize, AsRefStr, EnumString, BorshSerialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, export_to = "../../bindings/src/types/"))]
pub enum AbortReason {
    None,
    TransactionAtomMustBeAbort,
    TransactionAtomMustBeCommit,
    InputLockConflict,
    ForeignPledgeInputConflict,
    LockInputsFailed,
    LockOutputsFailed,
    LockInputsOutputsFailed,
    InvalidTransaction,
    ExecutionFailure,
    OneOrMoreInputsNotFound,
    ForeignShardGroupDecidedToAbort,
    InsufficientFeesPaid,
    EarlyAbort,
}

impl Display for AbortReason {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

impl From<&RejectReason> for AbortReason {
    fn from(reject_reason: &RejectReason) -> Self {
        match reject_reason {
            RejectReason::Unknown => Self::None,
            RejectReason::InvalidTransaction(_) => Self::InvalidTransaction,
            RejectReason::ExecutionFailure(_) => Self::ExecutionFailure,
            RejectReason::OneOrMoreInputsNotFound(_) => Self::OneOrMoreInputsNotFound,
            RejectReason::ForeignPledgeInputConflict => Self::ForeignPledgeInputConflict,
            RejectReason::FailedToLockInputs(_) => Self::LockInputsFailed,
            RejectReason::FailedToLockOutputs(_) => Self::LockOutputsFailed,
            RejectReason::ForeignShardGroupDecidedToAbort { .. } => Self::ForeignShardGroupDecidedToAbort,
            RejectReason::InsufficientFeesPaid(_) => Self::InsufficientFeesPaid,
        }
    }
}

impl Decision {
    pub fn is_commit(&self) -> bool {
        matches!(self, Decision::Commit)
    }

    pub fn is_abort(&self) -> bool {
        matches!(self, Decision::Abort(_))
    }

    pub fn and(self, other: Self) -> Self {
        match self {
            Decision::Commit => other,
            Decision::Abort(reason) => Decision::Abort(reason),
        }
    }

    pub fn abort_reason(&self) -> Option<AbortReason> {
        match self {
            Decision::Commit => None,
            Decision::Abort(reason) => Some(*reason),
        }
    }
}

impl Display for Decision {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Decision::Commit => write!(f, "Commit"),
            Decision::Abort(reason) => write!(f, "Abort({})", reason.as_ref()),
        }
    }
}

impl FromStr for Decision {
    type Err = FromStrConversionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Commit" => Ok(Decision::Commit),
            "Abort" => {
                // to stay compatible with previous messages
                Ok(Decision::Abort(AbortReason::None))
            },
            _ => {
                const ABORT_PREFIX: &str = "Abort(";
                // abort with reason
                if s.starts_with(ABORT_PREFIX) {
                    let start = ABORT_PREFIX.len();
                    let end = s
                        .rfind(')')
                        .ok_or_else(|| FromStrConversionError::InvalidDecision(s.to_string()))?;
                    let reason = &s[start..end];
                    return Ok(Decision::Abort(AbortReason::from_str(reason).map_err(|error| {
                        FromStrConversionError::InvalidAbortReason(s.to_string(), error)
                    })?));
                }

                Err(FromStrConversionError::InvalidDecision(s.to_string()))
            },
        }
    }
}

impl From<&TransactionResult> for Decision {
    fn from(result: &TransactionResult) -> Self {
        if result.is_accept() {
            Decision::Commit
        } else if let TransactionResult::Reject(reject_reason) = result {
            Decision::Abort(AbortReason::from(reject_reason))
        } else {
            Decision::Abort(AbortReason::None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_parses_decision_correctly() {
        let decision = Decision::Commit.to_string().parse::<Decision>().unwrap();
        assert_eq!(decision, Decision::Commit);

        let decision = Decision::Abort(AbortReason::None)
            .to_string()
            .parse::<Decision>()
            .unwrap();
        assert_eq!(decision, Decision::Abort(AbortReason::None));

        let decision = Decision::Abort(AbortReason::InvalidTransaction)
            .to_string()
            .parse::<Decision>()
            .unwrap();
        assert_eq!(decision, Decision::Abort(AbortReason::InvalidTransaction));
    }
}
