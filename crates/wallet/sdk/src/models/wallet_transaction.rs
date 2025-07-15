//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, str::FromStr, time::Duration};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tari_consensus_types::ProposalCertificate;
use tari_engine_types::commit_result::FinalizeResult;
use tari_transaction::{Transaction, TransactionId};
use time::PrimitiveDateTime;

use crate::models::NewAccountInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct WalletTransaction {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub id: TransactionId,
    pub transaction: Transaction,
    pub status: TransactionStatus,
    pub finalize: Option<FinalizeResult>,
    pub final_fee: Option<u64>,
    pub qcs: Vec<ProposalCertificate>,
    pub invalid_reason: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "{secs: number, nanos: number} | null"))]
    pub execution_time: Option<Duration>,
    #[cfg_attr(feature = "ts", ts(type = "{secs: number, nanos: number} | null"))]
    pub finalized_time: Option<Duration>,
    pub new_account_info: Option<NewAccountInfo>,
    pub is_dry_run: bool,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub last_update_time: PrimitiveDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum TransactionStatus {
    #[default]
    New,
    DryRun,
    DryRunFailed,
    Pending,
    Accepted,
    Rejected,
    InvalidTransaction,
    OnlyFeeAccepted,
}

impl TransactionStatus {
    pub fn as_key_str(&self) -> &'static str {
        match self {
            TransactionStatus::New => "New",
            TransactionStatus::DryRun => "DryRun",
            TransactionStatus::DryRunFailed => "DryRunFailed",
            TransactionStatus::Pending => "Pending",
            TransactionStatus::Accepted => "Accepted",
            TransactionStatus::Rejected => "Rejected",
            TransactionStatus::InvalidTransaction => "InvalidTransaction",
            TransactionStatus::OnlyFeeAccepted => "OnlyFeeAccepted",
        }
    }
}

impl FromStr for TransactionStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "New" => Ok(TransactionStatus::New),
            "DryRun" => Ok(TransactionStatus::DryRun),
            "DryRunFailed" => Ok(TransactionStatus::DryRunFailed),
            "Pending" => Ok(TransactionStatus::Pending),
            "Accepted" => Ok(TransactionStatus::Accepted),
            "Rejected" => Ok(TransactionStatus::Rejected),
            "InvalidTransaction" => Ok(TransactionStatus::InvalidTransaction),
            "OnlyFeeAccepted" => Ok(TransactionStatus::OnlyFeeAccepted),
            _ => Err(anyhow!("Invalid TransactionStatus: {}", s)),
        }
    }
}

impl Display for TransactionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_key_str())
    }
}

#[derive(Debug)]
pub struct WalletTransactionUpdate<'a> {
    pub transaction_id: TransactionId,
    pub result: Option<&'a FinalizeResult>,
    pub final_fee: Option<u64>,
    pub qcs: Option<&'a [ProposalCertificate]>,
    pub new_status: TransactionStatus,
    pub invalid_reason: Option<&'a str>,
    pub execution_time: Option<Duration>,
    pub finalized_time: Option<PrimitiveDateTime>,
}

impl<'a> WalletTransactionUpdate<'a> {
    pub fn new(transaction_id: TransactionId) -> Self {
        Self {
            transaction_id,
            result: None,
            final_fee: None,
            qcs: None,
            new_status: TransactionStatus::default(),
            invalid_reason: None,
            execution_time: None,
            finalized_time: None,
        }
    }

    pub fn with_result(mut self, result: Option<&'a FinalizeResult>) -> Self {
        self.result = result;
        self
    }

    pub fn with_final_fee(mut self, final_fee: Option<u64>) -> Self {
        self.final_fee = final_fee;
        self
    }

    pub fn with_qcs(mut self, qcs: &'a [ProposalCertificate]) -> Self {
        self.qcs = Some(qcs);
        self
    }

    pub fn with_new_status(mut self, new_status: TransactionStatus) -> Self {
        self.new_status = new_status;
        self
    }

    pub fn with_invalid_reason(mut self, invalid_reason: &'a str) -> Self {
        self.invalid_reason = Some(invalid_reason);
        self
    }

    pub fn with_execution_time(mut self, execution_time: Duration) -> Self {
        self.execution_time = Some(execution_time);
        self
    }

    pub fn with_finalized_time(mut self, finalized_time: PrimitiveDateTime) -> Self {
        self.finalized_time = Some(finalized_time);
        self
    }
}
