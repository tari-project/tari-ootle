//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{str::FromStr, time::Duration};

use chrono::NaiveDateTime;
use tari_dan_common_types::Epoch;
use tari_dan_wallet_sdk::{
    models::{TransactionStatus, WalletTransaction},
    storage::WalletStorageError,
};
use tari_transaction::{UnsealedTransactionV1, UnsignedTransactionV1};

use crate::{schema::transactions, serialization::deserialize_json};

#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = transactions)]
pub struct Transaction {
    pub id: i32,
    pub hash: String,
    pub network: i32,
    pub instructions: String,
    pub fee_instructions: String,
    pub inputs: String,
    pub signatures: String,
    pub seal_signature: String,
    pub is_seal_signer_authorized: bool,
    pub result: Option<String>,
    pub qcs: Option<String>,
    pub final_fee: Option<i64>,
    pub status: String,
    pub is_dry_run: bool,
    pub min_epoch: Option<i64>,
    pub max_epoch: Option<i64>,
    pub executed_time_ms: Option<i64>,
    pub finalized_time_ms: Option<i64>,
    pub required_substates: String,
    pub new_account_info: Option<String>,
    pub updated_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
}

impl Transaction {
    pub fn try_into_wallet_transaction(self) -> Result<WalletTransaction, WalletStorageError> {
        let signatures = deserialize_json(&self.signatures)?;
        let inputs = deserialize_json(&self.inputs)?;
        let seal_signature = deserialize_json(&self.seal_signature)?;

        Ok(WalletTransaction {
            transaction: tari_transaction::Transaction::new(
                UnsealedTransactionV1::new(
                    UnsignedTransactionV1 {
                        network: self.network.try_into().map_err(|_| WalletStorageError::DecodingError {
                            operation: "transaction_get",
                            item: "network",
                            details: format!("Invalid network value {}", self.network),
                        })?,
                        fee_instructions: deserialize_json(&self.fee_instructions)?,
                        instructions: deserialize_json(&self.instructions)?,
                        inputs,
                        min_epoch: self.min_epoch.map(|epoch| Epoch(epoch as u64)),
                        max_epoch: self.max_epoch.map(|epoch| Epoch(epoch as u64)),
                        is_seal_signer_authorized: true,
                    },
                    signatures,
                ),
                seal_signature,
            ),
            status: TransactionStatus::from_str(&self.status).map_err(|e| WalletStorageError::DecodingError {
                operation: "transaction_get",
                item: "status",
                details: e.to_string(),
            })?,
            finalize: self.result.as_deref().map(deserialize_json).transpose()?,
            final_fee: self.final_fee.map(|f| f.into()),
            qcs: self.qcs.map(|q| deserialize_json(&q)).transpose()?.unwrap_or_default(),
            required_substates: deserialize_json(&self.required_substates)?,
            new_account_info: self.new_account_info.as_deref().map(deserialize_json).transpose()?,
            is_dry_run: self.is_dry_run,
            execution_time: self
                .executed_time_ms
                .map(|t| u64::try_from(t).map(Duration::from_millis).unwrap_or_default()),
            finalized_time: self
                .finalized_time_ms
                .map(|t| u64::try_from(t).map(Duration::from_millis).unwrap_or_default()),
            last_update_time: self.updated_at,
        })
    }
}
