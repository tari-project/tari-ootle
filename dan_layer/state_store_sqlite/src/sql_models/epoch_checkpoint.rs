//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::Queryable;
use tari_dan_storage::{consensus_models, StorageError};
use time::PrimitiveDateTime;

use crate::serialization::deserialize_json;

#[derive(Debug, Clone, Queryable)]
pub struct EpochCheckpoint {
    pub id: i32,
    pub epoch: i64,
    pub proof: String,
    pub shard_roots: String,
    pub created_at: PrimitiveDateTime,
}

impl TryFrom<EpochCheckpoint> for consensus_models::EpochCheckpoint {
    type Error = StorageError;

    fn try_from(value: EpochCheckpoint) -> Result<Self, Self::Error> {
        let proof = deserialize_json(&value.proof)?;
        let shard_roots = deserialize_json(&value.shard_roots)?;

        Ok(Self::new(proof, shard_roots))
    }
}
