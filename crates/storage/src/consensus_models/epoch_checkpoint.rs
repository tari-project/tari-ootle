//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, iter};

use anyhow::anyhow;
use borsh::{BorshDeserialize, BorshSerialize};
use indexmap::IndexMap;
use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};
use tari_common_types::types::{CompressedPublicKey, FixedHash};
use tari_crypto::tari_utilities::ByteArray;
use tari_ootle_common_types::{Epoch, ShardGroup, VotePower, shard::Shard};
use tari_sidechain::{CommandCommitProof, SidechainBlockHeader, SidechainProofValidationError, ToCommand};
use tari_state_tree::{
    SPARSE_MERKLE_PLACEHOLDER_HASH,
    StateTreeError,
    TreeHash,
    Version,
    compute_merkle_root_for_hashes,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct EpochCheckpoint {
    #[n(0)]
    #[cbor(with = "tari_bor::adapters::serde_bridge")]
    proof: CommandCommitProof<EndOfEpochCommand>,
    #[n(1)]
    #[cbor(with = "tari_bor::adapters::indexmap_codec")]
    shard_tree_summary: IndexMap<Shard, TreeRootSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, CborLen)]
pub struct TreeRootSummary {
    #[n(0)]
    #[cbor(with = "tari_bor::adapters::serde_bridge")]
    pub root_hash: TreeHash,
    #[n(1)]
    pub state_version: Version,
}

impl EpochCheckpoint {
    pub fn new(
        proof: CommandCommitProof<EndOfEpochCommand>,
        shard_tree_summary: IndexMap<Shard, TreeRootSummary>,
    ) -> Self {
        Self {
            proof,
            shard_tree_summary,
        }
    }

    pub fn proof(&self) -> &CommandCommitProof<EndOfEpochCommand> {
        &self.proof
    }

    pub fn header(&self) -> &SidechainBlockHeader {
        self.proof.header()
    }

    pub fn checked_shard_group(&self) -> Result<ShardGroup, EpochCheckpointValidationError> {
        convert_sidechain_shard_group_to_shard_group(self.header().shard_group)
    }

    pub fn epoch(&self) -> Epoch {
        Epoch(self.proof.header().epoch)
    }

    pub fn shard_tree_summary(&self) -> &IndexMap<Shard, TreeRootSummary> {
        &self.shard_tree_summary
    }

    pub fn get_shard_root(&self, shard: Shard) -> TreeHash {
        self.shard_tree_summary
            .get(&shard)
            .map(|summary| summary.root_hash)
            .unwrap_or(SPARSE_MERKLE_PLACEHOLDER_HASH)
    }

    pub fn get_shard_state_version(&self, shard: Shard) -> Version {
        self.shard_tree_summary
            .get(&shard)
            .map(|summary| summary.state_version)
            .unwrap_or_default()
    }

    pub fn compute_state_merkle_root(&self) -> Result<TreeHash, EpochCheckpointValidationError> {
        let shard_group = self.checked_shard_group()?;
        let hashes = iter::once(Shard::global())
            .chain(shard_group.shard_iter())
            .map(|shard| self.get_shard_root(shard));
        let root = compute_merkle_root_for_hashes(hashes)?;
        Ok(root)
    }

    /// Validates that this epoch checkpoint is valid.
    /// 1. The commit proof is valid
    ///  - The command is included in the original block
    ///  - The QCs are signed by a correct quorum of validator nodes
    ///  - The 3-chain rule is satisfied
    /// 2. The state merkle root in the block header matches the provided shard hashes.
    ///  - Not to be confused with validating that some stored sharded state tree matches the epoch checkpoint - simply
    ///    a sanity check.
    pub fn validate(
        &self,
        epoch: Epoch,
        quorum_threshold: VotePower,
        check_vn: impl Fn(&RistrettoPublicKeyBytes) -> Result<VotePower, SidechainProofValidationError>,
    ) -> Result<(), EpochCheckpointValidationError> {
        if self.epoch() != epoch {
            return Err(EpochCheckpointValidationError::InvalidEpochCheckpoint(anyhow!(
                "Expected checkpoint epoch {} but proof epoch is {}",
                self.epoch(),
                self.proof.header().epoch
            )));
        }

        self.validate_well_formed()?;

        // Validate the proof
        self.proof
            .validate_committed(quorum_threshold.value() as usize, &|pk: &CompressedPublicKey| {
                let pk_bytes = RistrettoPublicKeyBytes::from_bytes(pk.as_bytes())
                    // Should not be possible - however, since CompressedPublicKey currently represented using a Vec<u8>
                    // it is possible, in theory, to have a CompressedPublicKey that is any size.
                    .map_err(SidechainProofValidationError::internal_error)?;
                // TODO: change this to return and check the actual voting power of the VN. Even if it is always 1 for
                // now.
                check_vn(&pk_bytes).map(|power| !power.is_zero())
            })?;

        Ok(())
    }

    /// Validates that the epoch checkpoint is well-formed.
    /// This includes basic sanity checks such as:
    /// - The shard group is valid (start < end + 1)
    /// - The number of shard state roots does not exceed the number of shards in the group
    /// - The state root matches the provided header
    ///
    /// Use EpochCheckpoint::validate() to validate the entire proof (including well-formedness).
    pub fn validate_well_formed(&self) -> Result<(), EpochCheckpointValidationError> {
        // Basic sanity checks
        let header_shard_group = convert_sidechain_shard_group_to_shard_group(self.header().shard_group)?;
        let num_shards = header_shard_group.len();
        if num_shards == 0 {
            return Err(EpochCheckpointValidationError::InvalidEpochCheckpoint(anyhow!(
                "Invalid shard group: end == start {}",
                header_shard_group
            )));
        }

        // 1 + for global shard
        if self.shard_tree_summary.len() > num_shards + 1 {
            return Err(
                EpochCheckpointValidationError::NumberOfShardStateRootsExceedsNumberOfShards {
                    num_shard_state_roots: self.shard_tree_summary.len(),
                    num_shards,
                },
            );
        }

        // TODO: more basic checks?

        // Validate state root matches provided header
        let state_root = self.compute_state_merkle_root()?;
        if state_root != self.proof.header().state_merkle_root {
            return Err(EpochCheckpointValidationError::ShardStateRootMerkleTreeRootMismatch {
                computed: state_root,
                header_state_root: self.proof.header().state_merkle_root,
            });
        }
        Ok(())
    }
}

impl EpochCheckpoint {
    pub fn get_all_from_epoch<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        from_epoch: Epoch,
        limit: usize,
    ) -> Result<Vec<Self>, StorageError> {
        tx.epoch_checkpoint_get_all_from_epoch(from_epoch, limit)
    }

    pub fn get_last_checkpoint<TTx: StateStoreReadTransaction>(tx: &TTx) -> Result<Self, StorageError> {
        tx.epoch_checkpoint_get_last()
    }

    pub fn get_by_shard_group<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        epoch: Epoch,
        shard_group: ShardGroup,
    ) -> Result<Self, StorageError> {
        tx.epoch_checkpoint_get_by_shard_group(epoch, shard_group)
    }

    pub fn save<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.epoch_checkpoint_save(self)
    }
}

impl Display for EpochCheckpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "EpochCheckpoint: block_id={}, epoch={}, count(shard_roots)={}",
            self.proof.header().calculate_block_id(),
            self.proof.header().epoch,
            self.shard_tree_summary.len()
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EpochCheckpointValidationError {
    #[error("Shard state root merkle tree root mismatch: computed {computed} != header state root {header_state_root}")]
    ShardStateRootMerkleTreeRootMismatch {
        computed: TreeHash,
        header_state_root: FixedHash,
    },
    #[error("Invalid state tree: {0}")]
    StateTreeError(#[from] StateTreeError),
    #[error("Sidechain proof validation error: {0}")]
    SidechainProofValidationError(#[from] SidechainProofValidationError),
    #[error("Number of shard state roots ({num_shard_state_roots}) exceeds number of shards ({num_shards})")]
    NumberOfShardStateRootsExceedsNumberOfShards {
        num_shard_state_roots: usize,
        num_shards: usize,
    },
    #[error("Invalid epoch checkpoint: {0}")]
    InvalidEpochCheckpoint(#[from] anyhow::Error),
}

fn convert_sidechain_shard_group_to_shard_group(
    shard_group: tari_sidechain::ShardGroup,
) -> Result<ShardGroup, EpochCheckpointValidationError> {
    ShardGroup::new_checked(shard_group.start, shard_group.end_inclusive).ok_or_else(|| {
        EpochCheckpointValidationError::InvalidEpochCheckpoint(anyhow!(
            "Invalid shard group: start >= end + 1 ({}-{})",
            shard_group.start,
            shard_group.end_inclusive
        ))
    })
}

#[derive(
    Debug, Clone, PartialEq, Eq, Deserialize, Serialize, BorshSerialize, BorshDeserialize, Encode, Decode, CborLen,
)]
pub struct EndOfEpochCommand;

impl ToCommand for EndOfEpochCommand {
    fn to_command(&self) -> tari_sidechain::Command {
        tari_sidechain::Command::EndEpoch
    }
}
