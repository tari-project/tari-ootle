//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::Queryable;
use tari_dan_common_types::VersionedSubstateId;
use tari_dan_storage::{
    consensus_models::{SubstatePledge, VersionedSubstateIdLockIntent},
    StorageError,
};
use tari_engine_types::substate::SubstateId;
use time::PrimitiveDateTime;

use crate::serialization::{deserialize_json, parse_from_string};

#[derive(Debug, Clone, Queryable)]
pub struct ForeignSubstatePledge {
    #[allow(dead_code)]
    pub id: i32,
    #[allow(dead_code)]
    pub transaction_id: String,
    #[allow(dead_code)]
    pub address: String,
    pub substate_id: String,
    pub version: i32,
    pub substate_value: Option<String>,
    #[allow(dead_code)]
    pub shard_group: i32,
    pub lock_type: String,
    #[allow(dead_code)]
    pub created_at: PrimitiveDateTime,
}

impl TryFrom<ForeignSubstatePledge> for SubstatePledge {
    type Error = StorageError;

    fn try_from(pledge: ForeignSubstatePledge) -> Result<Self, Self::Error> {
        let substate_id = parse_from_string::<SubstateId>(&pledge.substate_id)?;
        let version = pledge.version as u32;
        let id = VersionedSubstateId::new(substate_id, version);
        let lock_type = parse_from_string(&pledge.lock_type)?;
        let lock_intent = VersionedSubstateIdLockIntent::new(id, lock_type, true);
        let substate_value = pledge.substate_value.as_deref().map(deserialize_json).transpose()?;
        let pledge = SubstatePledge::try_create(lock_intent.clone(), substate_value).ok_or_else(|| {
            StorageError::DataInconsistency {
                details: format!("Invalid input substate pledge for {lock_intent}"),
            }
        })?;
        Ok(pledge)
    }
}
