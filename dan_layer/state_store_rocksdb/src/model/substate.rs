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
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{shard::Shard, Epoch, NodeHeight, SubstateAddress, VersionedSubstateId};
use tari_dan_storage::consensus_models::{BlockId, QcId, SubstateDestroyed, SubstateRecord};
use tari_engine_types::substate::{SubstateId, SubstateValue};
use tari_transaction::TransactionId;

use crate::{
    codecs::{
        DefaultCodec,
        EpochCodec,
        FixedBytesCodec,
        NumberCodec,
        ShardCodec,
        SubstateIdCodec,
        SubstateTransactionKeyCodec,
        TransactionIdCodec,
        UnitCodec,
    },
    traits::{Cf, QueryCf},
};

// We need to reimplement the "SubstateRecord" struct because of a incompatiblity between bincode and ciborium Value,
// which we use for the substate state.
// The error "Serde(AnyNotSupported)", due to bincode not being a self-describing format
// Ref: https://github.com/bincode-org/bincode/blob/trunk/src/features/serde/mod.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubstateModelData {
    pub substate_id: SubstateId,
    pub version: u32,
    pub substate_value: Option<Vec<u8>>,
    pub state_hash: FixedHash,
    pub created_by_transaction: TransactionId,
    pub created_justify: QcId,
    pub created_block: BlockId,
    pub created_height: NodeHeight,
    pub created_by_shard: Shard,
    pub created_at_epoch: Epoch,
    pub destroyed: Option<SubstateDestroyed>,
}

impl From<SubstateRecord> for SubstateModelData {
    fn from(rec: SubstateRecord) -> Self {
        SubstateModelData {
            substate_id: rec.substate_id,
            version: rec.version,
            substate_value: rec.substate_value.map(|v| v.to_bytes()),
            state_hash: rec.state_hash,
            created_by_transaction: rec.created_by_transaction,
            created_justify: rec.created_justify,
            created_block: rec.created_block,
            created_height: rec.created_height,
            created_by_shard: rec.created_by_shard,
            created_at_epoch: rec.created_at_epoch,
            destroyed: rec.destroyed,
        }
    }
}

impl TryFrom<SubstateModelData> for SubstateRecord {
    type Error = String;

    fn try_from(model: SubstateModelData) -> Result<Self, Self::Error> {
        let substate_value = match model.substate_value {
            Some(value) => {
                let value = SubstateValue::from_bytes(&value).map_err(|err| err.to_string())?;
                Some(value)
            },
            None => None,
        };

        Ok(SubstateRecord {
            substate_id: model.substate_id,
            version: model.version,
            substate_value,
            state_hash: model.state_hash,
            created_by_transaction: model.created_by_transaction,
            created_justify: model.created_justify,
            created_block: model.created_block,
            created_height: model.created_height,
            created_by_shard: model.created_by_shard,
            created_at_epoch: model.created_at_epoch,
            destroyed: model.destroyed,
        })
    }
}

pub struct SubstateModel;

impl Cf for SubstateModel {
    type Key = SubstateAddress;
    type KeyCodec = FixedBytesCodec<{ SubstateAddress::LENGTH }>;
    type Value = SubstateRecord;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "substates"
    }
}

pub struct HeadIndex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubstateHeadData {
    pub version: u32,
    pub is_up: bool,
}

impl Cf for HeadIndex {
    type Key = SubstateId;
    type KeyCodec = SubstateIdCodec;
    type Value = SubstateHeadData;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "substate_head_idx"
    }
}

pub struct TransactionIndex;

#[derive(Debug, Clone)]
pub struct SubstateTransactionIndexKey {
    pub transaction_id: TransactionId,
    pub versioned_substate_id: VersionedSubstateId,
    pub is_up: bool,
}

impl Cf for TransactionIndex {
    // (TransactionId, VersionedSubstateId, IsUp) where IsUp is true if the substate was upped by the transaction
    type Key = SubstateTransactionIndexKey;
    type KeyCodec = SubstateTransactionKeyCodec;
    type Value = ();
    type ValueCodec = UnitCodec;

    fn name() -> &'static str {
        "substate_transaction_idx"
    }
}

pub struct ByTransactionIdIndex;

impl QueryCf for ByTransactionIdIndex {
    type Cf = TransactionIndex;
    type Key = TransactionId;
    type KeyCodec = TransactionIdCodec;
}

pub struct UnprunedDownedValuesIndex;

impl Cf for UnprunedDownedValuesIndex {
    type Key = (Epoch, Shard, u64);
    type KeyCodec = (EpochCodec, ShardCodec, NumberCodec<u64>);
    type Value = <SubstateModel as Cf>::Key;
    type ValueCodec = <SubstateModel as Cf>::KeyCodec;

    fn name() -> &'static str {
        "substates_unpruned_idx"
    }
}

pub struct UnprunedDownedValuesEpochQuery;

impl QueryCf for UnprunedDownedValuesEpochQuery {
    type Cf = UnprunedDownedValuesIndex;
    type Key = Epoch;
    type KeyCodec = EpochCodec;
}
