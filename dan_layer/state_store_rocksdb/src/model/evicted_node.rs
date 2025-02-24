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
use tari_common_types::types::PublicKey;
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::consensus_models::BlockId;
use crate::{error::RocksDbStorageError, model::model::RocksdbModel, utils::RocksdbTimestamp};

use super::model::ModelColumnFamily;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvictedNodeData {
    pub public_key: PublicKey,
    pub evicted_in_block: BlockId,
    pub evicted_in_block_height: NodeHeight,
    pub epoch: Epoch,
    pub eviction_commmited_in_epoch: Option<Epoch>,

    // we need this field to differentiate between rows
    pub created_at: RocksdbTimestamp,
}

pub struct EvictedNodeModel {}

impl EvictedNodeModel {
    pub fn key_prefix_by_public_key(public_key: &PublicKey) -> String {
        format!("{}_{}_", Self::key_prefix(), public_key)
    }
}

impl RocksdbModel for EvictedNodeModel {
    type Item = EvictedNodeData;

    fn key_prefix() -> &'static str {
        "evictednodes"
    }

    fn key(value: &Self::Item) -> String {
        format!("{}_{}_{}", Self::key_prefix(), &value.public_key, value.created_at)
    }

    fn column_families() -> Vec<&'static str> {
        vec![EvictionCommittedColumnFamily::name()]
    }

    fn put_in_cfs(db: Arc<TransactionDB>, tx: &mut Transaction<'_, TransactionDB>, operation: &'static str, value: &Self::Item) -> Result<(), RocksDbStorageError> {
        // In each CF value We store the key to the main collection, so we can retrieve the actual value
        let main_key = Self::key(value);
        let main_key_bytes = main_key.as_bytes();

        EvictionCommittedColumnFamily::put(db.clone(), tx, operation,  value, main_key_bytes)?;

        Ok(())
    }
    
    fn delete_from_cfs(db: Arc<TransactionDB>, tx: &Transaction<'_, TransactionDB>, operation: &'static str, item: &Self::Item) -> Result<(), RocksDbStorageError> {
        EvictionCommittedColumnFamily::delete(db.clone(), tx, operation, item)?;
        Ok(())
    }
}

pub struct EvictionCommittedColumnFamily {}

impl EvictionCommittedColumnFamily {
    pub const NAME: &str = "evictednodes_eviction_committed";
    pub const UNCOMMITTED_VALUE: &str = "None";

    pub fn key_prefix_by_epoch(epoch: &Epoch) -> String {
        format!("{}_{}_", EvictedNodeModel::key_prefix(), epoch)
    }
}

impl ModelColumnFamily for EvictionCommittedColumnFamily {
    type Item = EvictedNodeData;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        let committed_in_epoch = value.eviction_commmited_in_epoch.map(|b| b.to_string()).unwrap_or(Self::UNCOMMITTED_VALUE.to_owned());
        format!("{}_{}_{}", EvictedNodeModel::key_prefix(), &committed_in_epoch, value.created_at)
    }
}