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
    pub const CF_BLOCK_ID: &str = "transactionexecution_block_id";

    pub fn cfs() -> Vec<&'static str> {
        vec![Self::CF_BLOCK_ID]
    }

    fn key(value: &Self) -> String {
        let transaction_id = value.transaction_execution.transaction_id();
        let block_id = value.transaction_execution.block_id();
        let created_at = value.created_at;
        // the key segment for "created_at" is binary to order keys by creation time
        format!("{}_{}_{}_{:b}", Self::KEY_PREFIX, transaction_id, block_id, created_at)
    }

    fn key_cf_block_id(value: &Self) -> String {
        let transaction_id = value.transaction_execution.transaction_id();
        let block_id = value.transaction_execution.block_id();
        // the block_id field is first to allow for prefix scanning
        format!("{}_{}_{}", Self::KEY_PREFIX, block_id, transaction_id)
    }

    pub fn key_prefix(block_id: &BlockId, transaction_id: &TransactionId) -> String {
        format!("{}_{}_{}_", Self::KEY_PREFIX, transaction_id, block_id)
    }

    pub fn key_prefix_by_transaction(transaction_id: &TransactionId) -> String {
        format!("{}_{}_", Self::KEY_PREFIX, transaction_id)
    }

    pub fn key_prefix_by_block(block_id: &BlockId) -> String {
        format!("{}_{}_", Self::KEY_PREFIX, block_id)
    }

    fn encode(value: &BlockTransactionExecutionModel) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
        Ok(bytes)
    }

    fn decode(bytes: Vec<u8>) -> Result<BlockTransactionExecutionModel, RocksDbStorageError> {
        let (value, _): (BlockTransactionExecutionModel, usize) = bincode::serde::decode_from_slice(&bytes, BINCODE_CONFIG)?;
        Ok(value)
    }

    pub fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &BlockTransactionExecution) -> Result<(), RocksDbStorageError> {
        let value = Self::from(value);
        let key: String = Self::key(&value);
        let encoded_value = Self::encode(&value)?;
        tx.put(&key, encoded_value)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        // block_id column family
        let cf = db.cf_handle(Self::CF_BLOCK_ID).unwrap();
        let key_cf = Self::key_cf_block_id(&value);
        // we put the main key as the value in the cf, so we can easily fetch the actual value later
        tx.put_cf(cf, key_cf, &key.as_bytes())
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
        })?;

        Ok(())
    }

    pub fn multi_get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, cf: &str, prefix: &str) -> Result<Vec<BlockTransactionExecutionModel>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        let prefix = format!("{}_{}", Self::KEY_PREFIX, prefix);
        options.set_iterate_range(rocksdb::PrefixRange(prefix.as_bytes()));

        let cf = db.cf_handle(cf).unwrap();

        let iterator = tx.iterator_cf_opt(cf,options, rocksdb::IteratorMode::Start);
        let values = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (_, key_bytes) = item.unwrap();
            let key = String::from_utf8(key_bytes.to_vec()).unwrap();
            let value = tx.get(&key)
                .map_err(|e| RocksDbStorageError::RocksDbError {
                    operation,
                    source: e,
                }).unwrap();
            let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation }).unwrap();
            Self::decode(bytes.to_vec()).unwrap()
        })
        .collect();

        Ok(values)
    }

    pub fn delete(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, value: &BlockTransactionExecutionModel) -> Result<(), RocksDbStorageError> {
        // delete the main value
        let key = Self::key(value);
        tx.delete(key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
            operation,
            source: e,
        })?;

        // delete the related block id CF value
        let cf = db.cf_handle(Self::CF_BLOCK_ID).unwrap();
        let key = Self::key_cf_block_id(value);
        tx.delete_cf(cf, key)
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