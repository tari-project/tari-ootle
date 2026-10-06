//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, ops::Deref};

use log::*;
use ootle_network::Network;
use serde::{Deserialize, Serialize};
use tari_consensus_types::BlockId;
use tari_engine_types::ProtocolVersion;
use tari_ootle_common_types::shard::Shard;
use tari_state_tree::{
    RootProofTree,
    SPARSE_MERKLE_PLACEHOLDER_HASH,
    SparseMerkleProofExt,
    SpreadPrefixStateTree,
    StateTreeError,
    TreeHash,
    Version,
    compute_merkle_root_for_hashes,
    key_mapper::{DbKeyMapper, HashIdentityKeyMapper},
    shard_state_leaf,
};

use crate::{
    ShardScopedTreeStoreReader,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{Block, CommittedBlockProof},
};

const LOG_TARGET: &str = "tari::ootle::storage::state_version_proof";

/// State sync proves streamed state at every shard state version that is a multiple of this. Every node that
/// serves a version stores proofs for these versions at least, so a stream relayed through a node that synced
/// it can still prove them.
pub const STATE_VERSION_PROOF_INTERVAL: Version = 32;

/// True if state sync must prove the shard state at `state_version`.
pub fn is_state_version_proof_point(state_version: Version) -> bool {
    state_version != 0 && state_version.is_multiple_of(STATE_VERSION_PROOF_INTERVAL)
}

/// What this node holds to prove that `shard` was at `state_version` in a committed block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateVersionProof {
    pub shard: Shard,
    pub state_version: Version,
    pub source: StateVersionProofSource,
    /// Proof of the shard's leaf (see [`shard_state_leaf`]) in the block's state merkle root.
    pub shard_root_proof: SparseMerkleProofExt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StateVersionProofSource {
    /// This node committed the block that produced the version, and generates its commit proof when serving it.
    Committed { block_id: BlockId },
    /// A CBOR-encoded [`CommittedBlockProof`] received from a peer during state sync and verified.
    Received { commit_proof: Vec<u8> },
}

/// Checks that `shard_root_proof` proves the leaf of `shard` at `shard_root` and `state_version` in the state
/// merkle root of `commit_proof`'s block, and that the block is one of `network` whose shard group holds `shard`.
///
/// Validating the commit proof's quorum certificates against the committee of its epoch and shard group is the
/// caller's part: this check only binds the shard state to the header those certificates sign.
pub fn verify_state_version_leaf(
    network: Network,
    commit_proof: &CommittedBlockProof,
    shard: Shard,
    state_version: Version,
    shard_root: &TreeHash,
    shard_root_proof: &SparseMerkleProofExt,
) -> Result<(), StateVersionProofError> {
    let header = &commit_proof.proof().header;
    if header.network != network.as_byte() {
        return Err(StateVersionProofError::WrongNetwork {
            network,
            header_network: header.network,
        });
    }
    let shard_group = commit_proof
        .shard_group()
        .map_err(|e| StateVersionProofError::InvalidHeader(e.to_string()))?;
    if !shard_group.contains_or_global(&shard) {
        return Err(StateVersionProofError::ShardNotInBlock {
            shard,
            shard_group: shard_group.to_string(),
        });
    }
    let protocol_version = commit_proof
        .protocol_version()
        .map_err(|e| StateVersionProofError::InvalidHeader(e.to_string()))?;
    // A leaf commits to the shard's state version only from V1. Before that it is the bare root, which proves no
    // version at all.
    if protocol_version < ProtocolVersion::V1 {
        return Err(StateVersionProofError::VersionNotCommitted { protocol_version });
    }
    let scheduled = ProtocolVersion::at(network, commit_proof.epoch());
    if protocol_version != scheduled {
        return Err(StateVersionProofError::InvalidHeader(format!(
            "block at epoch {} claims protocol {protocol_version}, but {network} runs {scheduled} there",
            commit_proof.epoch()
        )));
    }

    let leaf = shard_state_leaf(protocol_version, shard_root, state_version);
    let leaf_key = HashIdentityKeyMapper::map_to_leaf_key(&leaf);
    let state_merkle_root = TreeHash::new(commit_proof.state_merkle_root().into_array());
    shard_root_proof
        .verify_inclusion(&state_merkle_root, &leaf_key, &leaf)
        .map_err(|e| StateVersionProofError::LeafNotIncluded {
            shard,
            state_version,
            details: e.to_string(),
        })
}

#[derive(Debug, thiserror::Error)]
pub enum StateVersionProofError {
    #[error("proof is for a block of network byte {header_network}, not {network}")]
    WrongNetwork { network: Network, header_network: u8 },
    #[error("proof block runs protocol {protocol_version}, whose state root does not commit shard state versions")]
    VersionNotCommitted { protocol_version: ProtocolVersion },
    #[error("proof block header is invalid: {0}")]
    InvalidHeader(String),
    #[error("proof block of {shard_group} does not commit {shard}")]
    ShardNotInBlock { shard: Shard, shard_group: String },
    #[error("{shard} at v{state_version} with the streamed root is not in the proof block's state root: {details}")]
    LeafNotIncluded {
        shard: Shard,
        state_version: Version,
        details: String,
    },
}

/// Records, for every shard that `block` moved to a new state version, the proof of that shard's leaf in the
/// block's state merkle root, so that this node can prove the version to a peer that syncs it.
///
/// Must run once the block's tree diffs are committed, while the committed tree holds exactly the state the
/// block's root commits: every shard of the group at its latest committed root and version.
pub fn index_committed_block_state_versions<TTx>(
    tx: &mut TTx,
    block: &Block,
    version_updates: &HashMap<Shard, Version>,
) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
{
    let protocol_version = block.header().protocol_version();
    // Only a V1 or later root commits each shard's state version, so only it can prove one.
    if version_updates.is_empty() || protocol_version < ProtocolVersion::V1 {
        return Ok(());
    }
    let mut leaves = Vec::with_capacity(block.shard_group().len() + 1);
    let mut shard_leaves = HashMap::with_capacity(version_updates.len());
    for shard in block.shard_group().shard_iter_with_global() {
        let version = tx.state_tree_versions_get_latest(shard)?;
        let root = match version {
            Some(version) => {
                let mut store = ShardScopedTreeStoreReader::new(&**tx, shard);
                SpreadPrefixStateTree::new(&mut store)
                    .get_root_hash(version)
                    .map_err(|e| StorageError::QueryError {
                        reason: format!("root of {shard} at v{version}: {e}"),
                    })?
            },
            None => SPARSE_MERKLE_PLACEHOLDER_HASH,
        };
        let leaf = shard_state_leaf(protocol_version, &root, version.unwrap_or(0));
        if version_updates.contains_key(&shard) {
            shard_leaves.insert(shard, leaf);
        }
        leaves.push(leaf);
    }

    let to_storage_error = |e: StateTreeError| StorageError::QueryError {
        reason: format!("state version proofs for block {}: {e}", block.id()),
    };
    let root = compute_merkle_root_for_hashes(leaves.iter().copied()).map_err(to_storage_error)?;
    // The proofs are only served to syncing peers, so a node that cannot produce them keeps committing.
    if root.as_slice() != block.state_merkle_root().as_slice() {
        error!(
            target: LOG_TARGET,
            "BUG: committed state of {} has root {root}, but block {} commits {}. Not indexing its state versions.",
            block.shard_group(),
            block.id(),
            block.state_merkle_root()
        );
        return Ok(());
    }

    let root_tree = RootProofTree::build(leaves).map_err(to_storage_error)?;
    for (shard, state_version) in version_updates {
        let leaf = shard_leaves.get(shard).ok_or_else(|| StorageError::QueryError {
            reason: format!(
                "block {} updated {shard}, which is outside {}",
                block.id(),
                block.shard_group()
            ),
        })?;
        let (_, shard_root_proof) = root_tree.get_proof(*leaf).map_err(to_storage_error)?;
        tx.state_version_proofs_insert(&StateVersionProof {
            shard: *shard,
            state_version: *state_version,
            source: StateVersionProofSource::Committed { block_id: *block.id() },
            shard_root_proof,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::FixedHash;
    use tari_sidechain::{SidechainBlockCommitProof, SidechainBlockHeader};
    use tari_state_tree::SPARSE_MERKLE_PLACEHOLDER_HASH;

    use super::*;

    /// A commit proof for a block of `network` at epoch 1 under `protocol_version` whose state root holds the leaf of
    /// a shard at `shard_root` and `state_version`.
    fn proof_of_leaf(
        network: Network,
        protocol_version: ProtocolVersion,
        shard_root: TreeHash,
        state_version: Version,
    ) -> (CommittedBlockProof, SparseMerkleProofExt) {
        let leaf = shard_state_leaf(protocol_version, &shard_root, state_version);
        let leaves = vec![SPARSE_MERKLE_PLACEHOLDER_HASH, leaf];
        let root = compute_merkle_root_for_hashes(leaves.clone()).unwrap();
        let (_, shard_root_proof) = RootProofTree::build(leaves).unwrap().get_proof(leaf).unwrap();
        let header = SidechainBlockHeader {
            network: network.as_byte(),
            protocol_version: protocol_version.as_u32(),
            parent_id: FixedHash::zero(),
            justify_id: FixedHash::zero(),
            height: 1,
            epoch: 1,
            epoch_hash: FixedHash::zero(),
            shard_group: tari_sidechain::ShardGroup {
                start: 1,
                end_inclusive: 256,
            },
            proposed_by: Default::default(),
            state_merkle_root: FixedHash::from(root.into_array()),
            command_merkle_root: FixedHash::zero(),
            transaction_merkle_root: None,
            signature: Default::default(),
            accumulated_data: Default::default(),
            metadata_hash: FixedHash::zero(),
        };
        let commit_proof = CommittedBlockProof::new(SidechainBlockCommitProof {
            header,
            proof_elements: vec![],
        });
        (commit_proof, shard_root_proof)
    }

    #[test]
    fn a_v1_leaf_proves_its_state_version_only() {
        let root = TreeHash::new([7; 32]);
        let shard = Shard::from(3u32);
        let (commit_proof, leaf_proof) = proof_of_leaf(Network::LocalNet, ProtocolVersion::V1, root, 64);
        verify_state_version_leaf(Network::LocalNet, &commit_proof, shard, 64, &root, &leaf_proof).unwrap();
        assert!(matches!(
            verify_state_version_leaf(Network::LocalNet, &commit_proof, shard, 96, &root, &leaf_proof),
            Err(StateVersionProofError::LeafNotIncluded { .. })
        ));
    }

    #[test]
    fn a_v0_block_proves_no_state_version() {
        let root = TreeHash::new([7; 32]);
        let shard = Shard::from(3u32);
        let (commit_proof, leaf_proof) = proof_of_leaf(Network::Esmeralda, ProtocolVersion::V0, root, 64);
        for claimed in [64, 96] {
            assert!(matches!(
                verify_state_version_leaf(Network::Esmeralda, &commit_proof, shard, claimed, &root, &leaf_proof),
                Err(StateVersionProofError::VersionNotCommitted { .. })
            ));
        }
    }
}
