//  Copyright 2024. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{sync::Arc, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

use indexmap::IndexSet;
use rocksdb::{AsColumnFamilyRef, ColumnFamily, ColumnFamilyDescriptor, ColumnFamilyRef, Transaction, TransactionDB};
use serde::{Deserialize, Serialize};
use tari_dan_common_types::{NodeHeight, VersionedSubstateId};
use tari_dan_storage::{consensus_models::{Block, BlockId, BlockTransactionExecution, Decision, Evidence, LeaderFee, TransactionPoolRecord, TransactionPoolStage, TransactionPoolStatusUpdate, TransactionRecord, VersionedSubstateIdLockIntent}, Ordering};
use tari_engine_types::{commit_result::{ExecuteResult, RejectReason}, confidential::validate_elgamal_verifiable_balance_proof};
use tari_transaction::{TransactionId, TransactionSignature, UnsignedTransaction};
use tari_utilities::ByteArray;


use crate::error::RocksDbStorageError;

const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TransactionModel {
    // Needed as a workaround to avoid serialization problems with the tari_transaction::Transaction struct
    // as serde(flatten) is not supported in bincode: https://github.com/bincode-org/bincode/issues/245
    transaction: TransactionInner,
    execution_result: Option<ExecuteResult>,
    resulting_outputs: Option<Vec<VersionedSubstateIdLockIntent>>,
    resolved_inputs: Option<Vec<VersionedSubstateIdLockIntent>>,
    final_decision: Option<Decision>,
    finalized_time: Option<Duration>,
    abort_reason: Option<RejectReason>,
}

impl From<TransactionRecord> for TransactionModel {
    fn from(rec: TransactionRecord) -> Self {
        TransactionModel {
            transaction: rec.transaction.into(),
            execution_result: rec.execution_result,
            resulting_outputs: rec.resulting_outputs,
            resolved_inputs: rec.resolved_inputs,
            final_decision: rec.final_decision,
            finalized_time: rec.finalized_time,
            abort_reason: rec.abort_reason,
        }
    }
}

impl From<TransactionModel> for TransactionRecord {
    fn from(model: TransactionModel) -> Self {
        TransactionRecord {
            transaction: model.transaction.into(),
            execution_result: model.execution_result,
            resulting_outputs: model.resulting_outputs,
            resolved_inputs: model.resolved_inputs,
            final_decision: model.final_decision,
            finalized_time: model.finalized_time,
            abort_reason: model.abort_reason,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransactionInner {
    id: TransactionId,
    transaction: UnsignedTransaction,
    signatures: Vec<TransactionSignature>,
    filled_inputs: IndexSet<VersionedSubstateId>,
}

impl From<tari_transaction::Transaction> for TransactionInner {
    fn from(tx: tari_transaction::Transaction) -> Self {
        TransactionInner {
            id: *tx.id(),
            transaction: tx.unsigned_transaction().clone(),
            signatures: tx.signatures().to_vec(),
            filled_inputs: tx.filled_inputs().clone(),
        }
    }
}

impl From<TransactionInner> for tari_transaction::Transaction {
    fn from(tx: TransactionInner) -> Self {
        tari_transaction::Transaction::new(
            tx.transaction,
            tx.signatures,
        ).with_filled_inputs(tx.filled_inputs)
    }
}


impl TransactionModel {
    pub const KEY_PREFIX: &str = "transactions";

    fn key(tx_id: &TransactionId) -> String {
        format!("{}_{}", Self::KEY_PREFIX, tx_id.to_string())
    }

    fn encode(value: &TransactionRecord) -> Result<Vec<u8>, RocksDbStorageError> {
        let value = Self::from(value.clone());
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<TransactionRecord, RocksDbStorageError> {
        let (value, _): (TransactionModel, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        let value: TransactionRecord = value.into();
        Ok(value)
    }

    pub fn put(_db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &TransactionRecord) -> Result<(), RocksDbStorageError> {
        let key = Self::key(value.id());
        let value = Self::encode(value)?;
        tx.put(key, value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }

    pub fn get(tx: &Transaction<'_, TransactionDB>, operation: &'static str, tx_id: &TransactionId) -> Result<TransactionRecord, RocksDbStorageError> {
        let key = Self::key(tx_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
        let value = Self::decode(bytes)?;
        Ok(value)
    }

    pub fn multi_get(tx: &Transaction<'_, TransactionDB>, operation: &'static str, tx_ids: Vec<&TransactionId>) -> Result<Vec<TransactionRecord>, RocksDbStorageError> {
        let keys: Vec<String> = tx_ids
            .iter()
            .map(|id| Self::key(id))
            .collect();

        let values = tx.multi_get(keys)
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;

        let mut res = vec![];
        for value in values {
            if let Some(bytes) = value {
                let rec = Self::decode(bytes)?;
                res.push(rec);
            }
        }

        Ok(res)
    }

    pub fn key_exists(tx: &Transaction<'_, TransactionDB>, operation: &'static str, tx_id: &TransactionId) -> Result<bool, RocksDbStorageError> {
        let key = Self::key(tx_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        Ok(value.is_some())
    }

    pub fn list(tx: &Transaction<'_, TransactionDB>) -> Result<Vec<TransactionRecord>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        let key_prefix = format!("{}_", Self::KEY_PREFIX);
        options.set_iterate_range(rocksdb::PrefixRange(key_prefix.as_bytes()));

        let iterator = tx.iterator_opt( rocksdb::IteratorMode::Start, options);
        let res = iterator.map(|item| {
            let (_, bytes) = item.unwrap();
            Self::decode(bytes.to_vec()).unwrap()
        })
        .collect();

        Ok(res)
    }
}