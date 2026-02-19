//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use tari_ootle_common_types::engine_types::commit_result::RejectReason;

#[derive(Debug, Clone)]
pub enum TransactionOutcome {
    /// The transaction was fully committed.
    Commit,
    /// Only the fee intent was committed. The main transaction was not committed.
    OnlyFeeCommit(RejectReason),
    /// Transaction was rejected with the given reason. NOTE: calling get_receipt on the transaction will fail.
    Reject(RejectReason),
}

impl TransactionOutcome {
    pub fn is_commit(&self) -> bool {
        matches!(self, TransactionOutcome::Commit)
    }

    pub fn is_only_fee_commit(&self) -> bool {
        matches!(self, TransactionOutcome::OnlyFeeCommit(_))
    }

    pub fn is_reject(&self) -> bool {
        matches!(self, TransactionOutcome::Reject(_))
    }

    pub fn reject_reason(&self) -> Option<&RejectReason> {
        match self {
            TransactionOutcome::OnlyFeeCommit(reason) | TransactionOutcome::Reject(reason) => Some(reason),
            _ => None,
        }
    }
}

impl fmt::Display for TransactionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionOutcome::Commit => write!(f, "Commit"),
            TransactionOutcome::OnlyFeeCommit(reason) => write!(f, "OnlyFeeCommit({reason})"),
            TransactionOutcome::Reject(reason) => write!(f, "Reject({reason})"),
        }
    }
}
