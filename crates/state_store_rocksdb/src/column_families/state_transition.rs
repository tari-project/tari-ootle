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
use tari_ootle_common_types::{Epoch, SubstateAddress, shard::Shard};
use tari_state_tree::Version;

use crate::{
    codecs::{DefaultVersionedCodec, KeyPrefix, NumberCodec, ShardCodec},
    column_families::cf_names,
    prefixed,
    traits::{Cf, QueryCf, Versioned},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransitionModelDataV1 {
    pub epoch: Epoch,
    pub transitions: Vec<StateTransitionRecordData>,
    pub state_version: Version,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VersionedStateTransitionModelData {
    V1(StateTransitionModelDataV1),
}

impl Versioned for VersionedStateTransitionModelData {
    type Latest = StateTransitionModelDataV1;

    fn upgrade_single_step(self) -> (Self, bool) {
        match self {
            Self::V1(_) => (self, false), // No upgrades available
        }
    }

    fn into_latest(self) -> Self::Latest {
        match self {
            Self::V1(record) => record,
        }
    }
}

impl From<StateTransitionModelDataV1> for VersionedStateTransitionModelData {
    fn from(record: StateTransitionModelDataV1) -> Self {
        Self::V1(record)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransitionRecordData {
    pub substate_address: SubstateAddress,
    pub transition: StateTransitionType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StateTransitionType {
    Up,
    Down,
}

prefixed!(StateTransitionPrefix, KeyPrefix::StateTransitions);

pub struct StateTransitionCf;

impl Cf for StateTransitionCf {
    type Key = (Shard, Version);
    type KeyCodec = (ShardCodec, NumberCodec<Version>);
    type Prefix = StateTransitionPrefix;
    type Value = StateTransitionModelDataV1;
    type ValueCodec = DefaultVersionedCodec<VersionedStateTransitionModelData>;

    fn name() -> &'static str {
        cf_names::CHAIN_METADATA
    }
}

pub struct ByShardAndStateVersionQuery;

impl QueryCf for ByShardAndStateVersionQuery {
    type Cf = StateTransitionCf;
    type Key = (Shard, Version);
    type KeyCodec = (ShardCodec, NumberCodec<Version>);
}
