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
use tari_dan_common_types::{shard::Shard, NodeHeight};
use tari_dan_storage::consensus_models::{self, BlockId, VersionedStateHashTreeDiff};

use crate::{model::traits::RocksdbModel, utils::RocksdbSeq};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingStateTreeDiffData {
    pub block_id: BlockId,
    pub block_height: NodeHeight,
    pub shard: Shard,
    pub diff: VersionedStateHashTreeDiff,
}

impl From<PendingStateTreeDiffData> for consensus_models::PendingShardStateTreeDiff {
    fn from(value: PendingStateTreeDiffData) -> Self {
        let version = value.diff.version;
        Self::load(version, value.diff.diff)
    }
}

pub struct PendingStateTreeDiffModel {}

impl PendingStateTreeDiffModel {
    pub fn key_from_block_str_and_height(block_id: &str, height: Option<&NodeHeight>) -> String {
        let height = height
            .map(|h|
                // RocksdbSeq allows us to properly order keys by height
                RocksdbSeq(h.as_u64()).to_string())
            .unwrap_or_default();
        format!("{}_{}_{}", Self::key_prefix(), block_id, height)
    }

    pub fn key_from_block_and_height(block_id: &BlockId, height: Option<&NodeHeight>) -> String {
        Self::key_from_block_str_and_height(&block_id.to_string(), height)
    }
}

impl RocksdbModel for PendingStateTreeDiffModel {
    type Item = PendingStateTreeDiffData;

    fn key_prefix() -> &'static str {
        "pendingstatetreediff"
    }

    fn key(value: &Self::Item) -> String {
        Self::key_from_block_and_height(&value.block_id, Some(&value.block_height))
    }
}
