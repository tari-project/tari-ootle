//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::str::FromStr;

use diesel::Queryable;
use tari_dan_storage::{consensus_models, StorageError};
use time::PrimitiveDateTime;

use crate::serialization::{deserialize_hex_try_from, deserialize_json};

#[derive(Debug, Clone, Queryable)]
pub struct ForeignProposal {
    #[allow(dead_code)]
    pub id: i32,
    #[allow(dead_code)]
    pub block_id: String,
    #[allow(dead_code)]
    pub parent_block_id: String,
    #[allow(dead_code)]
    pub state_merkle_root: String,
    #[allow(dead_code)]
    pub command_merkle_root: String,
    #[allow(dead_code)]
    pub network: String,
    #[allow(dead_code)]
    pub height: i64,
    #[allow(dead_code)]
    pub epoch: i64,
    #[allow(dead_code)]
    pub shard_group: i32,
    #[allow(dead_code)]
    pub proposed_by: String,
    #[allow(dead_code)]
    pub command_count: i64,
    #[allow(dead_code)]
    pub commands: String,
    #[allow(dead_code)]
    pub metadata_hash: String,
    pub commit_proof: String,
    #[allow(dead_code)]
    pub signature: Option<String>,
    pub block_pledge: String,
    pub proposed_in_block: Option<String>,
    pub status: String,
    #[allow(dead_code)]
    pub created_at: PrimitiveDateTime,
}

impl ForeignProposal {
    pub fn try_convert(self) -> Result<consensus_models::ForeignProposalRecord, StorageError> {
        let block_id = deserialize_hex_try_from(&self.block_id)?;
        let status = consensus_models::ForeignProposalStatus::from_str(&self.status)?;
        let commit_proof = deserialize_json(&self.commit_proof)?;
        let block_pledge = deserialize_json(&self.block_pledge)?;
        let proposal = consensus_models::ForeignProposal::new(commit_proof, block_pledge);
        let proposed_in_block = self
            .proposed_in_block
            .as_deref()
            .map(deserialize_hex_try_from)
            .transpose()?;
        Ok(consensus_models::ForeignProposalRecord::load(
            block_id,
            proposal,
            proposed_in_block,
            status,
        ))
    }
}
