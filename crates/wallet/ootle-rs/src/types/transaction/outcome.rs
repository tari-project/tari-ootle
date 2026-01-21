//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use tari_ootle_common_types::engine_types::{commit_result::RejectReason, transaction_receipt::FinalizeOutcome};

#[derive(Debug, Clone)]
pub enum TransactionOutcome {
    /// The transaction was fully committed.
    Commit,
    /// Only the fee intent was committed. The main transaction was not committed.
    OnlyFeeCommit,
    /// Transaction was rejected with the given reason. NOTE: calling get_receipt on the transaction will fail.
    Reject(RejectReason),
}

impl TransactionOutcome {
    pub fn is_commit(&self) -> bool {
        matches!(self, TransactionOutcome::Commit | TransactionOutcome::OnlyFeeCommit)
    }

    pub fn is_only_fee_commit(&self) -> bool {
        matches!(self, TransactionOutcome::OnlyFeeCommit)
    }

    pub fn is_reject(&self) -> bool {
        matches!(self, TransactionOutcome::Reject(_))
    }
}

impl From<FinalizeOutcome> for TransactionOutcome {
    fn from(value: FinalizeOutcome) -> Self {
        match value {
            FinalizeOutcome::Commit => Self::Commit,
            FinalizeOutcome::FeeIntentCommit => Self::OnlyFeeCommit,
        }
    }
}

impl fmt::Display for TransactionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionOutcome::Commit => write!(f, "Commit"),
            TransactionOutcome::OnlyFeeCommit => write!(f, "OnlyFeeCommit"),
            TransactionOutcome::Reject(reason) => write!(f, "Reject({})", reason),
        }
    }
}
