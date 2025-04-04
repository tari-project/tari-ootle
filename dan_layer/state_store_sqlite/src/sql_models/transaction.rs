//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use diesel::{Queryable, Selectable};
use tari_dan_common_types::Epoch;
use tari_dan_storage::{consensus_models, consensus_models::Decision, StorageError};
use tari_transaction::{UnsealedTransactionV1, UnsignedTransactionV1};
use time::PrimitiveDateTime;

use crate::{schema::transactions, serialization::deserialize_json};

#[derive(Debug, Clone, Queryable, Selectable)]
pub struct Transaction {
    pub id: i32,
    pub network: i32,
    pub transaction_id: String,
    pub fee_instructions: String,
    pub instructions: String,
    pub inputs: String,
    pub filled_inputs: String,
    pub resolved_inputs: Option<String>,
    pub resulting_outputs: Option<String>,
    pub signatures: String,
    pub seal_signature: String,
    pub is_seal_signer_authorized: bool,
    pub result: Option<String>,
    pub execution_time_ms: Option<i64>,
    pub final_decision: Option<String>,
    pub finalized_at: Option<PrimitiveDateTime>,
    pub outcome: Option<String>,
    pub abort_details: Option<String>,
    pub min_epoch: Option<i64>,
    pub max_epoch: Option<i64>,
    pub schema_version: i64,
    pub created_at: PrimitiveDateTime,
}

impl TryFrom<Transaction> for tari_transaction::Transaction {
    type Error = StorageError;

    fn try_from(value: Transaction) -> Result<Self, Self::Error> {
        if value.schema_version != 1 {
            return Err(StorageError::DecodingError {
                operation: "TryFrom<Transaction> for tari_transaction::Transaction",
                item: "schema_version",
                details: format!("Unsupported schema version: {}", value.schema_version),
            });
        }

        let fee_instructions = deserialize_json(&value.fee_instructions)?;
        let instructions = deserialize_json(&value.instructions)?;
        let signatures = deserialize_json(&value.signatures)?;

        let inputs = deserialize_json(&value.inputs)?;

        let filled_inputs = deserialize_json(&value.filled_inputs)?;
        let min_epoch = value.min_epoch.map(|epoch| Epoch(epoch as u64));
        let max_epoch = value.max_epoch.map(|epoch| Epoch(epoch as u64));
        let seal_signature = deserialize_json(&value.seal_signature)?;
        let is_seal_signer_authorized = value.is_seal_signer_authorized;
        let network = value.network.try_into().map_err(|_| StorageError::DecodingError {
            operation: "TryFrom<Transaction> for tari_transaction::Transaction",
            item: "network",
            details: format!("Invalid network value {}", value.network),
        })?;

        Ok(Self::new(
            UnsealedTransactionV1::new(
                UnsignedTransactionV1 {
                    network,
                    fee_instructions,
                    instructions,
                    inputs,
                    min_epoch,
                    max_epoch,
                    is_seal_signer_authorized,
                },
                signatures,
            ),
            seal_signature,
        )
        .with_filled_inputs(filled_inputs))
    }
}

impl TryFrom<Transaction> for consensus_models::TransactionRecord {
    type Error = StorageError;

    fn try_from(value: Transaction) -> Result<Self, Self::Error> {
        let final_decision = value
            .final_decision
            .as_deref()
            .map(Decision::from_str)
            .transpose()
            .map_err(|_| StorageError::DecodingError {
                operation: "TryFrom<Transaction> for consensus_models::ExecutedTransaction",
                item: "decision",
                details: format!(
                    "Failed to parse decision from string: {}",
                    value.final_decision.as_ref().unwrap()
                ),
            })?;
        let result = value.result.as_deref().map(deserialize_json).transpose()?;
        let resulting_outputs = value.resulting_outputs.as_deref().map(deserialize_json).transpose()?;
        let resolved_inputs = value.resolved_inputs.as_deref().map(deserialize_json).transpose()?;
        let abort_details = value.abort_details.as_deref().map(deserialize_json).transpose()?;

        let finalized_time = value
            .finalized_at
            .map(|t| t.assume_offset(time::UtcOffset::UTC) - value.created_at.assume_offset(time::UtcOffset::UTC))
            .map(|d| d.try_into().unwrap_or_default());

        Ok(Self::load(
            value.try_into()?,
            result,
            resolved_inputs,
            final_decision,
            finalized_time,
            resulting_outputs,
            abort_details,
        ))
    }
}

impl TryFrom<Transaction> for consensus_models::ExecutedTransaction {
    type Error = StorageError;

    fn try_from(value: Transaction) -> Result<Self, Self::Error> {
        let rec = consensus_models::TransactionRecord::try_from(value)?;

        if rec.execution_result.is_none() {
            return Err(StorageError::QueryError {
                reason: format!("Transaction {} has not executed", rec.transaction.id()),
            });
        }
        rec.try_into()
    }
}
