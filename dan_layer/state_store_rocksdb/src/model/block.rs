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

use std::sync::Arc;

use rocksdb::{Transaction, TransactionDB};
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::{consensus_models::{Block, BlockId}, Ordering};

use crate::error::RocksDbStorageError;

use super::traits::{ModelColumnFamily, RocksdbModel};

pub struct BlockModel {}

impl BlockModel {
    pub fn key_from_block_id_str(block_id_str: &str) -> String {
        format!("{}_{}", Self::key_prefix(), block_id_str)
    }

    pub fn key_from_block_id(block_id: &BlockId) -> String {
        format!("{}_{}", Self::key_prefix(), block_id)
    }

    pub fn multi_get_ids_by_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, _operation: &'static str, cf: &str, prefix: &str) -> Result<Vec<BlockId>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_range(rocksdb::PrefixRange(prefix.as_bytes()));

        let cf = db.cf_handle(cf).unwrap();

        let iterator = tx.iterator_cf_opt(cf,options, rocksdb::IteratorMode::Start);
        let values = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (_, value) = item.unwrap();
            BlockId::try_from(value.to_vec()).unwrap()
        })
        .collect();

        Ok(values)
    }

    pub fn multi_get_ids_by_cf_range(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, _operation: &'static str, cf: &str, lower_prefix: &str, upper_prefix: &str) -> Result<Vec<BlockId>, RocksDbStorageError> {
        let mut options = rocksdb::ReadOptions::default();
        options.set_iterate_lower_bound(lower_prefix.as_bytes());
        options.set_iterate_upper_bound(upper_prefix.as_bytes());

        let cf = db.cf_handle(cf).unwrap();

        let iterator = tx.iterator_cf_opt(cf,options, rocksdb::IteratorMode::Start);
        let values = iterator.map(|item| {
            // TODO: properly handle errors and avoid unwraps
            let (_, value) = item.unwrap();
            BlockId::try_from(value.to_vec()).unwrap()
        })
        .collect();

        Ok(values)
    }
}

impl RocksdbModel for BlockModel {
    type Item = Block;

    fn key_prefix() -> &'static str {
        "blocks"
    }

    fn key(item: &Self::Item) -> String {
        Self::key_from_block_id(item.id())
    }

    fn get_cf(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, cf_name: &str, operation: &'static str, key_prefix_opt: Option<&str>, ordering: Ordering) -> Result<Option<Self::Item>, RocksDbStorageError> {
        let cf = db.cf_handle(cf_name).unwrap();

        let mut options = rocksdb::ReadOptions::default();

        let default_key_prefix = format!("{}_", Self::key_prefix());
        let key_prefix = key_prefix_opt.unwrap_or(&default_key_prefix);
        options.set_iterate_range(rocksdb::PrefixRange(key_prefix.as_bytes()));
       
        let iterator_mode = match ordering {
            Ordering::Ascending => rocksdb::IteratorMode::Start,
            Ordering::Descending => rocksdb::IteratorMode::End,
        };

        // get the block id from the CF
        let mut iterator = tx.iterator_cf_opt(cf, options, iterator_mode);
        let value = iterator.next()
            .ok_or_else(|| RocksDbStorageError::NotFound { key: key_prefix.to_string(), operation })?;
        let (_, bytes) = value.map_err(|e| RocksDbStorageError::RocksDbError {
            operation,
            source: e,
        })?;
        let block_id = BlockId::try_from(bytes.to_vec()).unwrap();

        // get the block
        let key = Self::key_from_block_id(&block_id);
        let value = tx.get(&key)
            .map_err(|e| RocksDbStorageError::RocksDbError {
                operation,
                source: e,
            })?;
        let bytes = value.ok_or_else(|| RocksDbStorageError::NotFound { key, operation })?;
        let value = Self::decode(bytes)?;
        Ok(Some(value))
    }

    fn column_families() -> Vec<&'static str> {
        vec![ParentIdColumnFamily::name(), EpochHeightColumnFamily::name(), IsCommittedColumnFamily::name()]
    }

    fn put_in_cfs(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &Self::Item) -> Result<(), RocksDbStorageError> {
        // In each CF value We store the BlockId of the block, so we can reference it back
        let cf_value = value.id().as_bytes();

        ParentIdColumnFamily::put(db.clone(), tx, operation,  value, cf_value)?;
        EpochHeightColumnFamily::put(db.clone(), tx, operation, value, cf_value)?;
        IsCommittedColumnFamily::put(db, tx, operation, value, cf_value)?;

        Ok(())
    }
    
    fn delete_from_cfs(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, item: &Self::Item) -> Result<(), RocksDbStorageError> {
        ParentIdColumnFamily::delete(db.clone(), tx, operation, item)?;
        EpochHeightColumnFamily::delete(db.clone(), tx, operation, item)?;
        IsCommittedColumnFamily::delete(db, tx, operation, item)?;
        Ok(())
    }
}

// Column family for Parent id
pub struct ParentIdColumnFamily {}

impl ParentIdColumnFamily {
    pub const NAME: &str = "blocks_parent_id";

    pub fn build_key_prefix(parent_id: &BlockId) -> String {
        format!("{}_{}_", BlockModel::key_prefix(), parent_id)
    }
}

impl ModelColumnFamily for ParentIdColumnFamily {
    type Item = Block;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        format!("{}_{}_{}", BlockModel::key_prefix(), value.parent(), value.id())
    }
}

// Column family for epoch-height
pub struct EpochHeightColumnFamily {}

impl EpochHeightColumnFamily {
    pub const NAME: &str = "blocks_epoch_height";

    pub fn build_key_prefix(epoch: Epoch, height: Option<NodeHeight>) -> String {
        match height {
            Some(height) => format!("{}_{}_{}_", BlockModel::key_prefix(), epoch, height),
            None => format!("{}_{}_", BlockModel::key_prefix(), epoch),
        }
    }
}

impl ModelColumnFamily for EpochHeightColumnFamily {
    type Item = Block;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        format!("{}_{}_{}_{}", BlockModel::key_prefix(), value.epoch(), value.height(), value.id())
    }
}

// Column family for "is_commited"
pub struct IsCommittedColumnFamily {}

impl IsCommittedColumnFamily {
    pub const NAME: &str = "blocks_is_committed";
}

impl ModelColumnFamily for IsCommittedColumnFamily {
    type Item = Block;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        format!("{}_{}", BlockModel::key_prefix(), value.id())
    }

    fn put(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, item: &Self::Item, _value: &[u8]) -> Result<(), RocksDbStorageError> {
        let key = Self::build_key(item);
        let cf = db.cf_handle(Self::name()).unwrap();
        if item.is_committed() {
            tx.put_cf(cf, key, item.id().as_bytes())
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
}