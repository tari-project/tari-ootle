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
use tari_dan_common_types::shard::Shard;
use tari_dan_storage::consensus_models::{StateTransition, StateTransitionId};

use super::{
    super::utils::bor_encode,
    traits::{ModelColumnFamily, RocksdbModel},
};
use crate::{error::RocksDbStorageError, utils::RocksdbSeq};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransitionModelData {
    pub data: Vec<u8>,
    pub id: StateTransitionId,
    pub shard: Shard,
    pub seq: RocksdbSeq,
}

impl StateTransitionModelData {
    pub fn new(data: StateTransition, shard: Shard, seq: u64) -> Result<Self, RocksDbStorageError> {
        let id = data.id;
        // we need to encode this field to bor due to incompatiblities between a inner SubstateValue field and bincode
        let data = bor_encode(&data)?;

        Ok(Self {
            data,
            id,
            shard,
            seq: RocksdbSeq(seq),
        })
    }
}

pub struct StateTransitionModel {}

impl RocksdbModel for StateTransitionModel {
    type Item = StateTransitionModelData;

    fn key_prefix() -> &'static str {
        "statetransitions"
    }

    fn key(item: &Self::Item) -> String {
        format!("{}_{}", Self::key_prefix(), item.id)
    }

    fn column_families() -> Vec<&'static str> {
        vec![ShardColumnFamily::name()]
    }

    fn put_in_cfs(
        db: Arc<TransactionDB>,
        tx: &mut Transaction<'_, TransactionDB>,
        operation: &'static str,
        value: &Self::Item,
    ) -> Result<(), RocksDbStorageError> {
        // In each CF value We store the key to the main collection, so we can retrieve the actual value
        let main_key = Self::key(value);
        let main_key_bytes = main_key.as_bytes();

        ShardColumnFamily::put(db, tx, operation, value, main_key_bytes)?;

        Ok(())
    }

    fn delete_from_cfs(
        db: Arc<TransactionDB>,
        tx: &Transaction<'_, TransactionDB>,
        operation: &'static str,
        item: &Self::Item,
    ) -> Result<(), RocksDbStorageError> {
        ShardColumnFamily::delete(db.clone(), tx, operation, item)?;

        Ok(())
    }
}

pub struct ShardColumnFamily {}

impl ShardColumnFamily {
    pub const NAME: &str = "statetransitions_shard_seq";

    pub fn build_key_prefix_by_shard(shard: Shard) -> String {
        format!("{}_{}_", StateTransitionModel::key_prefix(), shard)
    }
}

impl ModelColumnFamily for ShardColumnFamily {
    type Item = StateTransitionModelData;

    fn name() -> &'static str {
        Self::NAME
    }

    fn build_key(value: &Self::Item) -> String {
        format!("{}_{}_{}", StateTransitionModel::key_prefix(), value.shard, value.seq)
    }
}
