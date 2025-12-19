//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{str::FromStr, time::Duration};

use diesel::Identifiable;
use log::*;
use tari_ootle_common_types::displayable::Displayable;
use tari_ootle_wallet_sdk::{
    models::{TransactionStatus, WalletTransaction},
    storage::WalletStorageError,
};
use tari_transaction::Transaction;
use time::PrimitiveDateTime;

use crate::{
    schema::transactions,
    serialization::{deserialize_hex_try_from, deserialize_json},
};

const LOG_TARGET: &str = "tari::ootle::wallet::storage_sqlite::models::transaction";

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = transactions)]
pub struct TransactionRecord {
    pub id: i32,
    pub transaction_id: String,
    pub transaction_json: String,
    pub _referenced_components: String,
    pub _signers: String,
    pub result: Option<String>,
    pub qcs: Option<String>,
    pub final_fee: Option<i64>,
    pub status: String,
    pub dry_run: bool,
    pub executed_time_ms: Option<i64>,
    pub finalized_time: Option<PrimitiveDateTime>,
    pub new_account_info: Option<String>,
    pub invalid_reason: Option<String>,
    pub created_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

impl TransactionRecord {
    pub fn try_into_wallet_transaction(self) -> Result<WalletTransaction, WalletStorageError> {
        let transaction = deserialize_json::<Transaction, _>(&self.transaction_json)?;
        let is_dry_run = transaction.is_dry_run();

        Ok(WalletTransaction {
            id: deserialize_hex_try_from(&self.transaction_id)?,
            transaction,
            status: TransactionStatus::from_str(&self.status).map_err(|e| WalletStorageError::DecodingError {
                operation: "transaction_get",
                item: "status",
                details: e.to_string(),
            })?,
            finalize: self.result.as_deref().map(deserialize_json).transpose()?,
            final_fee: self.final_fee.map(|f| f as u64),
            qcs: self.qcs.map(|q| deserialize_json(&q)).transpose()?.unwrap_or_default(),
            new_account_info: self.new_account_info.as_deref().map(deserialize_json).transpose()?,
            invalid_reason: self.invalid_reason,
            is_dry_run,
            execution_time: self
                .executed_time_ms
                .map(|t| u64::try_from(t).map(Duration::from_millis).unwrap_or_default()),
            finalized_time: self
                .finalized_time
                .map(|t| t - self.created_at)
                .map(Duration::try_from)
                .transpose()
                .inspect_err(|e| {
                    warn!(
                        target: LOG_TARGET,
                        "Failed to convert finalized time to duration {} - {}: {}",
                        self.finalized_time.display(),
                        self.created_at,
                        e
                    );
                })
                // Negative duration cap to 0
                .unwrap_or_else(|_| Some(Duration::from_secs(0))),
            last_update_time: self.updated_at,
        })
    }
}
