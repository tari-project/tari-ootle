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

use serde::{Deserialize, Serialize};
use tari_dan_storage::consensus_models::{self, BlockId, SubstateChange};
use tari_engine_types::substate::SubstateId;
use crate::{error::RocksDbStorageError, model::model::RocksdbModel, utils::{bor_decode, bor_encode, RocksdbTimestamp}};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDiffData {
    pub block_id: BlockId,
    pub substate_id: SubstateId,
    pub change: SubstateChange,
    // we need this field to keep track of insertion order
    pub created_at: RocksdbTimestamp,
}

impl BlockDiffData {
    pub fn load(block_id: BlockId, diff: Vec<Self>) -> consensus_models::BlockDiff {
        let changes = diff
            .into_iter()
            .map(|d| d.change)
            .collect::<Vec<_>>();
        consensus_models::BlockDiff { block_id, changes }
    }
}

pub struct BlockDiffModel {}

impl BlockDiffModel {
    pub fn build_key_prefix_str(block_id: &str, substate_id_opt: Option<&SubstateId>) -> String {
        match substate_id_opt {
            Some(substate_id) => format!("{}_{}_{}_", Self::key_prefix(), block_id, substate_id),
            None => format!("{}_{}_", Self::key_prefix(), block_id),
        }
        
    }

    pub fn build_key_prefix(block_id: BlockId, substate_id_opt: Option<&SubstateId>) -> String {
        Self::build_key_prefix_str(&block_id.to_string(), substate_id_opt)    
    }
}

impl RocksdbModel for BlockDiffModel {
    type Item = BlockDiffData;

    fn key_prefix() -> &'static str {
        "blockdiffs"
    }

    fn key(value: &Self::Item) -> String {
        format!("{}_{}_{}_{}", Self::key_prefix(), value.block_id, value.substate_id, value.created_at)
    }

    // We need to override the default trait implementations to encode with tari_bor to avoid a bincode conflict with SubstateChange
    fn encode(value: &Self::Item) -> Result<Vec<u8>, RocksDbStorageError> {
        let bytes = bor_encode(value)?;
        Ok(bytes)
    }
    fn decode(bytes: Vec<u8>) -> Result<Self::Item, RocksDbStorageError> {
        let value: Self::Item = bor_decode(&bytes)?;
        Ok(value)
    }
}