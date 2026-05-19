//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_ootle_storage::consensus_models::SubstateRecord;

use crate::traits::Versioned;

pub type LatestSubstateRecord = SubstateRecord;

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub enum VersionedSubstateRecord {
    #[n(0)]
    V1(#[n(0)] SubstateRecord),
}

impl Versioned for VersionedSubstateRecord {
    type Latest = LatestSubstateRecord;

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

impl From<SubstateRecord> for VersionedSubstateRecord {
    fn from(record: SubstateRecord) -> Self {
        Self::V1(record)
    }
}

#[cfg(test)]
mod tests {
    use tari_bor::Value;
    use tari_engine_types::{
        component::{Component, ComponentBody, ComponentHeader},
        substate::SubstateValue,
    };
    use tari_ootle_common_types::{Epoch, shard::Shard};
    use tari_ootle_storage::consensus_models::{SubstateCreated, SubstateDestroyed};
    use tari_template_lib_types::{Hash32, SubstateOwnerRule};

    use super::*;
    use crate::codecs::{DbDecoder, DbEncoder, DefaultCodec};

    #[test]
    fn encode_decode() {
        let codec = DefaultCodec::new();
        let original = SubstateRecord {
            substate_id: "component_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .parse()
                .unwrap(),
            version: 0,
            substate_value: Some(SubstateValue::Component(Component {
                header: ComponentHeader {
                    template_address: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .parse()
                        .unwrap(),
                    owner_rule: SubstateOwnerRule::None,
                    access_rules: Default::default(),
                    entity_id: Default::default(),
                },
                body: ComponentBody { state: Value::Null },
            })),
            state_hash: Hash32::from_hex("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef").unwrap(),
            created: SubstateCreated {
                at_epoch: Epoch(123),
                in_shard: Shard::first(),
                at_state_version: 2,
            },
            destroyed: Some(SubstateDestroyed {
                at_epoch: Epoch(456),
                at_state_version: 5,
            }),
        };
        let versioned: VersionedSubstateRecord = original.into();
        let encoded = codec.encode(&versioned).expect("Encoding failed");
        let decoded: VersionedSubstateRecord = codec.decode_exact(&encoded).expect("Decoding failed");
        let _latest: LatestSubstateRecord = decoded.into_latest();
    }
}
