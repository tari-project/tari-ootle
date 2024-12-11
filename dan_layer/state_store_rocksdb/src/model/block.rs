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

pub(crate) struct BlockModel {}

impl BlockModel {
    pub const KEY_PREFIX: &str = "blocks";
    pub const CF_PARENT_ID: &str = "blocks_parent_id";
    pub const CF_EPOCH_HEIGHT: &str = "blocks_epoch_height";
    pub const CF_IS_COMMITED: &str = "blocks_is_committed";

    pub fn cfs() -> Vec<&'static str> {
        vec![Self::CF_PARENT_ID, Self::CF_EPOCH_HEIGHT, Self::CF_IS_COMMITED]
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

    pub fn get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, cf: &str, operation: &'static str, key_prefix: &str, ordering: Ordering) -> Result<Block, RocksDbStorageError> {
        let cf = db.cf_handle(cf).unwrap();
        let key_prefix = format!("{}_{}", Self::KEY_PREFIX, key_prefix);

        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange(key_prefix.as_bytes()));
       
        let iterator_mode = match ordering {
            Ordering::Ascending => rocksdb::IteratorMode::Start,
            Ordering::Descending => rocksdb::IteratorMode::End,
        };

        // get the block id from the CF
        let mut iterator = tx.iterator_cf_opt(cf, options, iterator_mode);
        let value = iterator.next()
            .ok_or_else(|| RocksDbStorageError::NotFound { key: key_prefix, operation })?;
        let (_, bytes) = value.map_err(|e| RocksDbStorageError::RocksDbError {
            operation,
            source: e,
        })?;
        let block_id = BlockId::try_from(bytes.to_vec()).unwrap();

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

        // blocks_is_committed column family
        let cf = db.cf_handle(Self::CF_IS_COMMITED).unwrap();
        let key = format!("{}_{}", Self::KEY_PREFIX, block.id());
        if block.is_committed() {
            tx.put_cf(cf, key, block.id().as_bytes())
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        } else {
            tx.delete_cf(cf, key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        }      

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