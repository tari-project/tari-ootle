//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, iter};

use anyhow::anyhow;
use borsh::{BorshDeserialize, BorshSerialize};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tari_common_types::types::{CompressedPublicKey, FixedHash};
use tari_crypto::tari_utilities::ByteArray;
use tari_ootle_common_types::{shard::Shard, Epoch, ShardGroup};
use tari_sidechain::{CommandCommitProof, SidechainBlockHeader, SidechainProofValidationError, ToCommand};
use tari_state_tree::{compute_merkle_root_for_hashes, StateTreeError, TreeHash, SPARSE_MERKLE_PLACEHOLDER_HASH};
use tari_template_lib::prelude::RistrettoPublicKeyBytes;

use crate::{StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochCheckpoint {
    proof: CommandCommitProof<EndOfEpochCommand>,
    shard_roots: IndexMap<Shard, TreeHash>,
}

impl EpochCheckpoint {
    pub fn new(proof: CommandCommitProof<EndOfEpochCommand>, shard_roots: IndexMap<Shard, TreeHash>) -> Self {
        Self { proof, shard_roots }
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

    pub fn shard_roots(&self) -> &IndexMap<Shard, TreeHash> {
        &self.shard_roots
    }

    pub fn get_shard_root(&self, shard: Shard) -> TreeHash {
        self.shard_roots
            .get(&shard)
            .copied()
            .unwrap_or(SPARSE_MERKLE_PLACEHOLDER_HASH)
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
        quorum_threshold: usize,
        check_vn: impl Fn(&RistrettoPublicKeyBytes) -> Result<bool, SidechainProofValidationError>,
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
            .validate_committed(quorum_threshold, &|pk: &CompressedPublicKey| {
                let pk_bytes = RistrettoPublicKeyBytes::from_bytes(pk.as_bytes())
                    // Should not be possible - however, since CompressedPublicKey currently represented using a Vec<u8>
                    // it is possible, in theory, to have a CompressedPublicKey that is any size.
                    .map_err(SidechainProofValidationError::internal_error)?;
                check_vn(&pk_bytes)
            })?;

        Ok(())
    }

    fn validate_well_formed(&self) -> Result<(), EpochCheckpointValidationError> {
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
        if self.shard_roots().len() > num_shards + 1 {
            return Err(
                EpochCheckpointValidationError::NumberOfShardStateRootsExceedsNumberOfShards {
                    num_shard_state_roots: self.shard_roots().len(),
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
    pub fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.epoch_checkpoint_get(epoch)
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
            self.shard_roots.len()
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EpochCheckpointValidationError {
    #[error(
        "Shard state root merkle tree root mismatch: computed {computed} != header state root {header_state_root}"
    )]
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, BorshSerialize, BorshDeserialize)]
pub struct EndOfEpochCommand;

impl ToCommand for EndOfEpochCommand {
    fn to_command(&self) -> tari_sidechain::Command {
        tari_sidechain::Command::EndEpoch
    }
}
