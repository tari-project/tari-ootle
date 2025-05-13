//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt,
    fmt::{Display, Formatter},
};

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};
use strum::ParseError;
use tari_engine_types::commit_result::{AbortReason, TransactionResult};

#[derive(Debug, Clone, thiserror::Error)]
pub enum FromStrConversionError {
    #[error("Invalid Decision string '{0}'")]
    InvalidDecision(String),
    #[error("Invalid Abort reason string '{0}': {1}")]
    InvalidAbortReason(String, ParseError),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize, BorshSerialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
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
            Decision::Abort(reason) => write!(f, "Abort({:?})", reason),
        }
    }
}

impl From<&TransactionResult> for Decision {
    fn from(result: &TransactionResult) -> Self {
        match result {
            TransactionResult::Accept(_) | TransactionResult::AcceptFeeRejectRest(_, _) => Decision::Commit,
            TransactionResult::Reject(reason) => Decision::Abort(AbortReason::from(reason)),
        }
    }
}
