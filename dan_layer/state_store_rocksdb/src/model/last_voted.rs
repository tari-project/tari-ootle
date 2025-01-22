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
use tari_dan_storage::consensus_models::LastVoted;
use crate::{error::RocksDbStorageError, model::model::RocksdbModel, utils::RocksdbTimestamp};

use super::model::ModelColumnFamily;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastVotedModelData {
    pub last_voted: LastVoted,
    // we need this field to keep track of insertion order
    pub created_at: RocksdbTimestamp,
}

impl From<&LastVoted> for LastVotedModelData {
    fn from(value: &LastVoted) -> Self {
        Self {
            last_voted: value.clone(),
            created_at: RocksdbTimestamp::now(),
        }
    }
}

pub struct LastVotedModel {}

impl RocksdbModel for LastVotedModel {
    type Item = LastVotedModelData;

    fn key_prefix() -> &'static str {
        "lastvoted"
    }

    fn key(value: &Self::Item) -> String {
        format!("{}_{}_{}", Self::key_prefix(), value.last_voted.block_id, value.last_voted.height)
    }

    fn column_families() -> Vec<&'static str> {
        vec![TimestampColumnFamily::name()]
    }

    fn put_in_cfs(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &Self::Item) -> Result<(), RocksDbStorageError> {
        // In each CF value We store the key to the main collection, so we can retrieve the actual value
        let main_key = Self::key(value);
        let main_key_bytes = main_key.as_bytes();

        TimestampColumnFamily::put(db.clone(), tx, operation,  value, main_key_bytes)?;

        Ok(())
    }

    fn delete_from_cfs(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, item: &Self::Item) -> Result<(), RocksDbStorageError> {
        TimestampColumnFamily::delete(db.clone(), tx, operation, item)?;
        
        Ok(())
    }
}

pub struct TimestampColumnFamily {}

impl ModelColumnFamily for TimestampColumnFamily {
    type Item = LastVotedModelData;

    fn name() -> &'static str {
        "lastvoted_timestamp"
    }

    fn build_key(value: &Self::Item) -> String {
        // the key segment for "created_at" allows us to order by creation time
        format!("{}_{}", LastVotedModel::key_prefix(), value.created_at)
    }
}