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
use serde::{Deserialize, Serialize};
use tari_dan_storage::consensus_models::{BlockId, BlockTransactionExecution};
use tari_transaction::TransactionId;
use crate::{model::model::RocksdbModel, utils::RocksdbTimestamp};

use crate::error::RocksDbStorageError;

use super::model::ModelColumnFamily;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockTransactionExecutionModelData {
    pub transaction_execution: BlockTransactionExecution,
    // we need this field to keep track of insertion order
    // for the "transaction_executions_get_pending_for_block" method
    pub created_at: RocksdbTimestamp,
}

impl From<&BlockTransactionExecution> for BlockTransactionExecutionModelData {
    fn from(exec: &BlockTransactionExecution) -> Self {
        Self {
            transaction_execution: exec.clone(),
            created_at: RocksdbTimestamp::now(),
        }
    }
}

pub struct BlockTransactionExecutionModel {}

impl BlockTransactionExecutionModel {
    pub fn key_prefix_by_transaction_and_block(transaction_id: &TransactionId, block_id_opt: Option<&BlockId>) -> String {
        match block_id_opt {
            Some(block_id) => format!("{}_{}_{}_", Self::key_prefix(), transaction_id, block_id),
            None => format!("{}_{}_", Self::key_prefix(), transaction_id),
        }
    }
}

impl RocksdbModel for BlockTransactionExecutionModel {
    type Item = BlockTransactionExecutionModelData;

    fn key_prefix() -> &'static str {
        "transactionexecution"
    }

    fn key(value: &Self::Item) -> String {
        let transaction_id = value.transaction_execution.transaction_id();
        let block_id = value.transaction_execution.block_id();
        let created_at = value.created_at;
        // the key segment for "created_at" allows us to order by creation time
        format!("{}_{}_{}_{}", Self::key_prefix(), transaction_id, block_id, created_at)
    }

    fn column_families() -> Vec<&'static str> {
        vec![BlockColumnFamily::name()]
    }

    fn put_in_cfs(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &Self::Item) -> Result<(), RocksDbStorageError> {
        // In each CF value We store the key to the main collection, so we can retrieve the actual value
        let main_key = Self::key(value);
        let main_key_bytes = main_key.as_bytes();

        BlockColumnFamily::put(db.clone(), tx, operation,  value, main_key_bytes)?;

        Ok(())
    }

    fn delete_from_cfs(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, item: &Self::Item) -> Result<(), RocksDbStorageError> {
        BlockColumnFamily::delete(db.clone(), tx, operation, item)?;
        
        Ok(())
    }
}

// destroyed by transaction
pub struct BlockColumnFamily {}

impl BlockColumnFamily {
    pub const NAME: &str = "transactionexecution_block_id";

    pub fn key_prefix_by_block(block_id: &BlockId) -> String {
        format!("{}_{}_", BlockTransactionExecutionModel::key_prefix(), block_id)
    }
}

impl ModelColumnFamily for BlockColumnFamily {
    type Item = BlockTransactionExecutionModelData;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        let transaction_id = value.transaction_execution.transaction_id();
        let block_id = value.transaction_execution.block_id();
        // the block_id field is first to allow for prefix scanning
        format!("{}_{}_{}", BlockTransactionExecutionModel::key_prefix(), block_id, transaction_id)
    }
}