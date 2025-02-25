//  Copyright 2025. The Tari Project
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
use serde::{Deserialize, Serialize};
use tari_dan_storage::consensus_models::{BlockId, LockConflict};
use tari_transaction::TransactionId;
use crate::{error::RocksDbStorageError, model::traits::RocksdbModel, utils::RocksdbTimestamp};

use super::traits::ModelColumnFamily;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockConflictData {
    pub block_id: BlockId,
    pub conflict: LockConflict,
    pub transaction_id: TransactionId,
    pub depends_on_tx: TransactionId,
    // we need this field to differentiate between conflicts of the same transaction
    pub created_at: RocksdbTimestamp,
}

pub struct LockConflictModel {}

impl RocksdbModel for LockConflictModel {
    type Item = LockConflictData;

    fn key_prefix() -> &'static str {
        "lockconflicts"
    }

    fn key(value: &Self::Item) -> String {
        format!("{}_{}_{}", Self::key_prefix(), &value.conflict.transaction_id, value.created_at)
    }

    fn column_families() -> Vec<&'static str> {
        vec![BlockIdColumnFamily::name()]
    }

    fn put_in_cfs(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &Self::Item) -> Result<(), RocksDbStorageError> {
        // In each CF value We store the key to the main collection, so we can retrieve the actual value
        let main_key = Self::key(value);
        let main_key_bytes = main_key.as_bytes();

        BlockIdColumnFamily::put(db.clone(), tx, operation,  value, main_key_bytes)?;

        Ok(())
    }
    
    fn delete_from_cfs(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, item: &Self::Item) -> Result<(), RocksDbStorageError> {
        BlockIdColumnFamily::delete(db.clone(), tx, operation, item)?;
        Ok(())
    }
}

// block id
pub struct BlockIdColumnFamily {}

impl BlockIdColumnFamily {
    pub const NAME: &str = "lockconflicts_block_id";

    pub fn build_key_prefix_by_block(block_id: &BlockId) -> String {
        format!("{}_{}_", LockConflictModel::key_prefix(), block_id)
    }
}

impl ModelColumnFamily for BlockIdColumnFamily {
    type Item = LockConflictData;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        format!("{}_{}_{}", LockConflictModel::key_prefix(), value.block_id, value.conflict.transaction_id)
    }
}