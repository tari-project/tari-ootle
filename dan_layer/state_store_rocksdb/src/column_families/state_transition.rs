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

use serde::{Deserialize, Serialize};
use tari_dan_common_types::{shard::Shard, SubstateAddress};
use tari_dan_storage::consensus_models::StateTransitionId;

use crate::{
    codecs::{DefaultCodec, EpochCodec, NumberCodec, ShardCodec, StateTransitionIdCodec},
    traits::{Cf, QueryCf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransitionModelData {
    pub substate_address: SubstateAddress,
    pub transition: StateTransitionType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StateTransitionType {
    Up,
    Down,
}

pub struct StateTransitionCf;

impl Cf for StateTransitionCf {
    type Key = StateTransitionId;
    type KeyCodec = StateTransitionIdCodec<ShardCodec, NumberCodec<u64>, EpochCodec>;
    type Value = StateTransitionModelData;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "state_transitions"
    }
}

pub struct ByShardQuery;

impl QueryCf for ByShardQuery {
    type Cf = StateTransitionCf;
    type Key = Shard;
    type KeyCodec = ShardCodec;
}

pub struct ByShardAndIdQuery;

impl QueryCf for ByShardAndIdQuery {
    type Cf = StateTransitionCf;
    type Key = (Shard, u64);
    type KeyCodec = (ShardCodec, NumberCodec<u64>);
}

pub struct ShardSeqIndex;

impl Cf for ShardSeqIndex {
    type Key = Shard;
    type KeyCodec = ShardCodec;
    type Value = u64;
    type ValueCodec = NumberCodec<Self::Value>;

    fn name() -> &'static str {
        "state_transition_shard_seq_idx"
    }
}
