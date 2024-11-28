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
pub(crate) struct TransactionPoolStateUpdateModel {
    pub block_id: BlockId,
    pub block_height: NodeHeight,
    pub transaction_id: TransactionId,
    pub evidence: Evidence,
    pub transaction_fee: u64,
    pub leader_fee: Option<LeaderFee>,
    pub stage: TransactionPoolStage,
    pub local_decision: Decision,
    pub remote_decision: Option<Decision>,
    pub is_ready: bool,
    pub is_applied: bool,
}

impl TransactionPoolStateUpdateModel {
    pub const KEY_PREFIX: &str = "transactionpoolpendingupdate_";

    fn key(value: &TransactionPoolStateUpdateModel) -> String {
        // the key format allows us to query all updates by block prefix, ordered by height DESC
        let block_id =  value.block_id.to_string();
        // TODO: is there a cleaner way to implement desc key ordering in RocksDb?
        let block_height_desc = NodeHeight(u64::MAX) - value.block_height;
        let tx_id = value.transaction_id.to_string();
        format!("{}_{}_{}_{}", Self::KEY_PREFIX, block_id, block_height_desc, tx_id)
    }

    fn encode(value: &TransactionPoolStateUpdateModel) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<TransactionPoolStateUpdateModel, RocksDbStorageError> {
        let (value, _): (TransactionPoolStateUpdateModel, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(value)
    }

    pub fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &TransactionPoolStateUpdateModel) -> Result<(), RocksDbStorageError> {
        let key = Self::key(value);
        let value_bytes = Self::encode(value)?;
        tx.put(key, value_bytes)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }

    pub fn multi_get(tx: &Transaction<'_, TransactionDB>, _operation: &'static str, prefix: &str) -> Result<Vec<TransactionPoolStateUpdateModel>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        let prefix = format!("{}_{}", Self::KEY_PREFIX, prefix);
        options.set_iterate_range(rocksdb::PrefixRange(prefix.as_bytes()));

        let iterator = tx.iterator_opt(rocksdb::IteratorMode::Start, options);
        let values = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (_, value) = item.unwrap();
            let value = Self::decode(value.to_vec()).unwrap();
            value
        })
        .collect();
        Ok(values)
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TransactionPoolPendingUpdateModel {
    pub transaction: TransactionPoolRecord,
    pub block_id: BlockId,
    pub block_height: NodeHeight,
    pub is_applied: bool,
}

impl TransactionPoolPendingUpdateModel {
    pub const KEY_PREFIX: &str = "transactionpoolpendingupdate_";
    //pub const CF_BLOCK_ID: &str = "transactionpoolpendingupdate_block_id";

    fn key(tx_id: &TransactionId) -> String {
        format!("transactionpoolpendingupdate_{}", tx_id.to_string())
    }

    fn encode(value: &TransactionPoolPendingUpdateModel) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<TransactionPoolPendingUpdateModel, RocksDbStorageError> {
        let (value, _): (TransactionPoolPendingUpdateModel, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(value)
    }

    pub fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &TransactionPoolPendingUpdateModel) -> Result<(), RocksDbStorageError> {
        let key = Self::key(value.transaction.transaction_id());
        let value_bytes = Self::encode(value)?;
        tx.put(key, value_bytes)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        /*
        // blocks_id column family
        let cf = db.cf_handle(Self::CF_BLOCK_ID).unwrap();
        let key_cf = Self::key_cf(&value.block_id.to_string());
        tx.put_cf(cf, key_cf, value.transaction.transaction_id().as_bytes())
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;
         */

        Ok(())
    }

    /*
    pub fn multi_get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, cf: &str, operation: &'static str, key_value: &str) -> Result<Vec<TransactionPoolPendingUpdateModel>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        let prefix = format!("{}{}", Self::KEY_PREFIX, key_value);
        options.set_iterate_range(rocksdb::PrefixRange(prefix.as_bytes()));

        // get all the tx_ids from the column family
        let cf = db.cf_handle(cf).unwrap();
        let iterator = tx.iterator_cf_opt(cf, options, rocksdb::IteratorMode::Start);
        let tx_ids: Vec<TransactionId> = iterator.map(|item: Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>| {
            // TODO: properly handle errors and avoid unwraps
            let (_, value) = item.unwrap();
            TransactionId::try_from(value.to_vec()).unwrap()
        })
        .collect();
        
        // get all the values
        let mut updates = vec![];
        for tx_id in tx_ids {
            let key = Self::key(&tx_id);
            let value = tx.get(&key)
                .map_err(|e| RocksDbStorageError::RocksDbError {
                    operation,
                    source: e,
                })?;
            let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
            let update = Self::decode(bytes)?;
            updates.push(update);
        }

        Ok(updates)
    }
     */
}

pub(crate) struct TransactionPoolModel {}

impl TransactionPoolModel {
    fn key(tx_id: &TransactionId) -> String {
        format!("transactionpool_{}", tx_id.to_string())
    }

    fn encode(value: &TransactionPoolRecord) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<TransactionPoolRecord, RocksDbStorageError> {
        let (value, _): (TransactionPoolRecord, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(value)
    }

    pub fn put(tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &TransactionPoolRecord) -> Result<(), RocksDbStorageError> {
        let key = Self::key(value.transaction_id());
        let value = Self::encode(value)?;
        tx.put(key, value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }

    pub fn get(tx: &Transaction<'_, TransactionDB>, operation: &'static str, tx_id: &TransactionId) -> Result<TransactionPoolRecord, RocksDbStorageError> {
        let key = Self::key(tx_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
        let block = Self::decode(bytes)?;
        Ok(block)
    }

    pub fn get_all(tx: &Transaction<'_, TransactionDB>, operation: &'static str) -> Result<Vec<TransactionPoolRecord>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange("transactionpool_".as_bytes()));
        let iterator = tx.iterator_opt(rocksdb::IteratorMode::Start, options);
        let values = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (key, value) = item.unwrap();
            let value = Self::decode(value.to_vec()).unwrap();
            value
        })
        .collect();
        Ok(values)
    }

    pub fn try_convert(
        value: &TransactionPoolRecord,
        update: Option<TransactionPoolStateUpdateModel>,
    ) -> Result<TransactionPoolRecord, RocksDbStorageError> {
        let mut evidence = value.evidence().clone();
        let mut pending_stage = None;
        let mut local_decision = value.local_decision();
        let mut is_ready = value.is_ready();
        let mut remote_decision = value.remote_decision();
        let mut leader_fee = value.leader_fee().cloned();
        let mut transaction_fee = value.transaction_fee();

        if let Some(update) = update {
            evidence = update.evidence;
            is_ready = update.is_ready;
            pending_stage = Some(update.stage);
            local_decision = Some(update.local_decision);
            remote_decision = update.remote_decision;
            leader_fee = update.leader_fee;
            transaction_fee = update.transaction_fee;
        }

        Ok(TransactionPoolRecord::load(
            *value.transaction_id(),
            evidence,
            transaction_fee as u64,
            leader_fee,
            value.stage(),
            pending_stage,
            value.original_decision(),
            local_decision,
            remote_decision,
            is_ready,
        ))
    }
}

pub(crate) struct BlockModel {}

impl BlockModel {
    pub const KEY_PREFIX: &str = "blocks";
    pub const CF_PARENT_ID: &str = "blocks_parent_id";
    pub const CF_EPOCH_HEIGHT: &str = "blocks_epoch_height";

    pub fn cfs() -> Vec<&'static str> {
        vec![Self::CF_PARENT_ID, Self::CF_EPOCH_HEIGHT]
    }

    fn key(block_id: &BlockId) -> String {
        format!("{}_{}", Self::KEY_PREFIX, block_id.to_string())
    }

    fn encode(block: &Block) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(block, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<Block, RocksDbStorageError> {
        let (block, _): (Block, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(block)
    }

    pub fn key_exists(tx: &Transaction<'_, TransactionDB>, operation: &'static str, block_id: &BlockId) -> Result<bool, RocksDbStorageError> {
        let key = Self::key(block_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        Ok(value.is_some())
    }

    pub fn get(tx: &Transaction<'_, TransactionDB>, operation: &'static str, block_id: &BlockId) -> Result<Block, RocksDbStorageError> {
        let key = Self::key(block_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
        let block = Self::decode(bytes)?;
        Ok(block)
    }

    pub fn get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, cf: &str, operation: &'static str, key_prefix: &str) -> Result<Block, RocksDbStorageError> {
        let cf = db.cf_handle(cf).unwrap();
        let key = format!("{}_{}", Self::KEY_PREFIX, key_prefix);
        let value = tx.get_cf(cf, &key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
        let block_id = BlockId::try_from(bytes).unwrap();

        // get the block
        let key = Self::key(&block_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;

        let block = Self::decode(bytes)?;
        Ok(block)
    }

    pub fn multi_get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, _operation: &'static str, cf: &str, prefix: &str) -> Result<Vec<BlockId>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        let prefix = format!("{}_{}", Self::KEY_PREFIX, prefix);
        options.set_iterate_range(rocksdb::PrefixRange(prefix.as_bytes()));

        let cf = db.cf_handle(cf).unwrap();

        let iterator = tx.iterator_cf_opt(cf,options, rocksdb::IteratorMode::Start);
        let values = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (_, value) = item.unwrap();
            let value = BlockId::try_from(value.to_vec()).unwrap();
            value
        })
        .collect();

        Ok(values)
    }

    pub fn multi_get_cf_range(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, _operation: &'static str, cf: &str, lower_prefix: &str, upper_prefix: &str) -> Result<Vec<BlockId>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        let lower = format!("{}_{}", Self::KEY_PREFIX, lower_prefix);
        options.set_iterate_lower_bound(lower.as_bytes());
        let upper = format!("{}_{}", Self::KEY_PREFIX, upper_prefix);
        options.set_iterate_upper_bound(upper.as_bytes());

        let cf = db.cf_handle(cf).unwrap();

        let iterator = tx.iterator_cf_opt(cf,options, rocksdb::IteratorMode::Start);
        let values = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (_, value) = item.unwrap();
            let value = BlockId::try_from(value.to_vec()).unwrap();
            value
        })
        .collect();

        Ok(values)
    }

    pub fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, block: &Block) -> Result<(), RocksDbStorageError> {
        let key = Self::key(block.id());
        let value = Self::encode(block)?;

        // put the whole block in the default column family
        tx.put(key, value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        // blocks_parent_id column family
        let cf = db.cf_handle(Self::CF_PARENT_ID).unwrap();
        let key = format!("{}_{}_{}", Self::KEY_PREFIX, block.parent(), block.id());
        tx.put_cf(cf, key, block.id().as_bytes())
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        // blocks_epoch_height column family
        let cf = db.cf_handle(Self::CF_EPOCH_HEIGHT).unwrap();
        let key = format!("{}_{}_{}_{}", Self::KEY_PREFIX, block.epoch(), block.height(), block.id());
        tx.put_cf(cf, key, block.id().as_bytes())
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;         

        Ok(())
    }

    pub fn delete(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, block_id: &BlockId) -> Result<(), RocksDbStorageError> {
        let key = Self::key(block_id);

        // we need to fetch the parent id for CF related keys
        let block = BlockModel::get(tx, operation, block_id)?;

        tx.delete(key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
            operation,
            source: e,
        })?;

        // we also need to delete related CF keys
        let cf = db.cf_handle(Self::CF_PARENT_ID).unwrap();
        let key = format!("{}_{}_{}", Self::KEY_PREFIX, block.parent(), block.id());
        tx.delete_cf(cf, key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }

    pub fn list(tx: &Transaction<'_, TransactionDB>) -> Result<Vec<Block>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange("blocks_".as_bytes()));

        let iterator = tx.iterator_opt( rocksdb::IteratorMode::Start, options);
        let blocks = iterator.map(|item| {
            let (_, bytes) = item.unwrap();
            Self::decode(bytes.to_vec()).unwrap()
        })
        .collect();

        Ok(blocks)
    }

    pub fn count(tx: &Transaction<'_, TransactionDB>) -> Result<i64, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange("blocks_".as_bytes()));
        let iterator = tx.iterator_opt(rocksdb::IteratorMode::Start, options);
        let count = iterator.count() as i64;
        Ok(count)
    }
}


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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BlockTransactionExecutionModel {
    pub transaction_execution: BlockTransactionExecution,
    // we need this field to keep track of insertion order
    // for the "transaction_executions_get_pending_for_block" method
    pub created_at: u128,
}

impl From<&BlockTransactionExecution> for BlockTransactionExecutionModel {
    fn from(exec: &BlockTransactionExecution) -> Self {
        Self {
            transaction_execution: exec.clone(),
            created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),
        }
    }
}

impl BlockTransactionExecutionModel {
    pub const KEY_PREFIX: &str = "transactionexecution_";

    fn key(value: &Self) -> String {
        let transaction_id = value.transaction_execution.transaction_id();
        let block_id = value.transaction_execution.block_id();
        let created_at = value.created_at;
        // the key segment for "created_at" is binary to order keys by creation time
        format!("{}_{}_{}_{:b}", Self::KEY_PREFIX, transaction_id, block_id, created_at)
    }

    pub fn key_prefix(block_id: &BlockId, transaction_id: &TransactionId) -> String {
        format!("{}_{}_{}_", Self::KEY_PREFIX, transaction_id, block_id)
    }

    pub fn key_prefix_by_transaction(transaction_id: &TransactionId) -> String {
        format!("{}_{}_", Self::KEY_PREFIX, transaction_id)
    }

    fn encode(value: &BlockTransactionExecutionModel) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<BlockTransactionExecutionModel, RocksDbStorageError> {
        let (value, _): (BlockTransactionExecutionModel, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(value)
    }

    pub fn put(_db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &BlockTransactionExecution) -> Result<(), RocksDbStorageError> {
        let value = Self::from(value);
        let key = Self::key(&value);
        let value = Self::encode(&value)?;
        tx.put(key, value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }

    pub fn list(tx: &Transaction<'_, TransactionDB>, key_prefix: &str, ordering: Ordering) -> Result<Vec<BlockTransactionExecutionModel>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange(key_prefix.as_bytes()));
       
        let iterator_mode = match ordering {
            Ordering::Ascending => rocksdb::IteratorMode::Start,
            Ordering::Descending => rocksdb::IteratorMode::End,
        };

        let iterator = tx.iterator_opt(iterator_mode, options);

        let res = iterator.map(|item| {
            let (_, bytes) = item.unwrap();
            let model = Self::decode(bytes.to_vec()).unwrap();
            model
        })
        .collect();

        Ok(res)
    }

    pub fn key_exists(tx: &Transaction<'_, TransactionDB>, tx_id: &TransactionId, block_id: &BlockId) -> Result<bool, RocksDbStorageError> {
        let key_prefix = Self::key_prefix(block_id, tx_id);

        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange(key_prefix.as_bytes()));
        let mut iterator = tx.iterator_opt(rocksdb::IteratorMode::Start, options);
        
        Ok(iterator.next().is_some())
    }
}

