//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use diesel::Queryable;
use tari_dan_storage::{consensus_models, StorageError};
use time::PrimitiveDateTime;

use crate::serialization::deserialize_json;

#[derive(Debug, Clone, Queryable)]
pub struct ForeignParkedBlock {
    pub id: i32,
    pub block_id: String,
    pub commit_proof: String,
    pub block_pledges: String,
    pub created_at: PrimitiveDateTime,
}

impl TryFrom<ForeignParkedBlock> for consensus_models::ForeignParkedProposal {
    type Error = StorageError;

    fn try_from(value: ForeignParkedBlock) -> Result<Self, Self::Error> {
        let commit_proof = deserialize_json(&value.commit_proof)?;
        let block_pledge = deserialize_json(&value.block_pledges)?;

        Ok(consensus_models::ForeignParkedProposal::new(commit_proof, block_pledge))
    }
}
