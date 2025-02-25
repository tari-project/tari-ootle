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

use tari_common_types::types::FixedHash;
use tari_dan_storage::consensus_models::{BlockId, Vote};
use crate::model::traits::RocksdbModel;

pub struct VoteModel {}

impl VoteModel {
    pub fn key_from_block_and_sender(block_id: &BlockId, sender_leaf_hash_opt: Option<&FixedHash>) -> String {
        let sender_leaf_hash = sender_leaf_hash_opt.map(|h| h.to_string()).unwrap_or_default();
        format!("{}_{}_{}", Self::key_prefix(), block_id, sender_leaf_hash)
    }
}

impl RocksdbModel for VoteModel {
    type Item = Vote;

    fn key_prefix() -> &'static str {
        "votes"
    }

    fn key(value: &Self::Item) -> String {
        Self::key_from_block_and_sender(&value.block_id, Some(&value.sender_leaf_hash))
    }
}