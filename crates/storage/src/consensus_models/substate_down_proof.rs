//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use log::*;
use serde::{Deserialize, Serialize};
use tari_consensus_types::BlockId;
use tari_ootle_common_types::{ToSubstateAddress, VersionedSubstateId, optional::Optional, shard::Shard};
use tari_state_tree::{
    SparseMerkleProofExt,
    SpreadPrefixStateTree,
    SubstateDownProof,
    SubstateValueProof,
    TreeHash,
    Version,
};

use crate::{
    ShardScopedTreeStoreReader,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{DownedSubstate, StateVersionProofSource, SubstateRecord},
};

const LOG_TARGET: &str = "tari::ootle::storage::substate_down_proof";

/// How far below the destroying state version to look for a version to prove the substate up at.
///
/// The state tree keeps the nodes of a bounded number of recent versions, so a version further back than that cannot
/// produce a leaf proof, and the substate was up at every version in between.
const MAX_UP_VERSION_LOOKBACK: Version = 128;

/// The half of a [`SubstateDownProof`] that stops being producible once the substate goes down: proof that it was
/// committed at shard state version `state_version`, with the commit proof of the block whose root holds that state.
///
/// Written in the same write transaction that commits the substate's destruction, while the tree nodes at
/// `state_version` and the block that committed it still exist. It is self-contained so that it stays servable after
/// both are pruned. The other half, the exclusion proof, is generated when it is served, against the latest root.
///
/// A record is a few KB: a level-1 leaf proof, a level-2 shard-root proof and a commit proof of three certificates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubstateDownProofRecord {
    /// The shard state version `u` the substate is proved up at, lower than the version that destroyed it.
    pub state_version: Version,
    /// The substate's leaf value hash at `u`.
    pub value_hash: TreeHash,
    /// Level-1 inclusion of the substate in the shard tree at `u`.
    pub leaf_proof: SparseMerkleProofExt,
    /// The shard root at `u`.
    pub shard_root: TreeHash,
    /// Level-2 proof of the shard's leaf in the state merkle root of `commit_proof`'s block.
    pub shard_root_proof: SparseMerkleProofExt,
    /// CBOR-encoded `CommittedBlockProof` of the block whose root commits the shard at `u`.
    pub commit_proof: Vec<u8>,
}

impl SubstateDownProofRecord {
    /// Completes the proof with `down`, the exclusion proof of the substate under the root of the commit proof the
    /// proof is served with.
    pub fn into_down_proof(self, down: SubstateValueProof) -> SubstateDownProof {
        SubstateDownProof {
            up: SubstateValueProof::new(
                self.shard_root,
                self.state_version,
                self.shard_root_proof,
                self.leaf_proof,
            ),
            up_value_hash: self.value_hash,
            up_commit_proof: self.commit_proof,
            down,
        }
    }
}

/// Records, for each substate in `downed`, the proof that it was committed before it went down (see
/// [`SubstateDownProofRecord`]).
///
/// Must run in the write transaction that commits the destructions, after the state tree diffs are committed.
/// `commit_proof_for_block` returns the encoded commit proof of a block this node committed, or `None` when it can no
/// longer build one (e.g. the block was pruned). A substate for which no proof can be assembled - such as on a node
/// that state-synced past every version it was up at - gets no record, and is served as an unproven Down.
pub fn index_substate_down_proofs<TTx, F>(
    tx: &mut TTx,
    downed: &[DownedSubstate],
    mut commit_proof_for_block: F,
) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    F: FnMut(&TTx::Target, &BlockId) -> Option<Vec<u8>>,
{
    for downed in downed {
        match build_down_proof_record(&**tx, downed, &mut commit_proof_for_block)? {
            Some(record) => tx.substate_down_proofs_insert(downed.shard, &downed.id, &record)?,
            None => {
                debug!(
                    target: LOG_TARGET,
                    "No provable up version for {} destroyed at {} v{}; it will be served without a down proof",
                    downed.id,
                    downed.shard,
                    downed.state_version,
                );
            },
        }
    }
    Ok(())
}

/// The record for the greatest version below the destroying one that this node can prove the substate up at.
fn build_down_proof_record<TTx, F>(
    tx: &TTx,
    downed: &DownedSubstate,
    commit_proof_for_block: &mut F,
) -> Result<Option<SubstateDownProofRecord>, StorageError>
where
    TTx: StateStoreReadTransaction,
    F: FnMut(&TTx, &BlockId) -> Option<Vec<u8>>,
{
    let Some(highest) = downed.state_version.checked_sub(1) else {
        return Ok(None);
    };
    let Some(substate) = SubstateRecord::get(tx, &downed.id.to_substate_address()).optional()? else {
        return Ok(None);
    };
    let lowest = substate
        .created()
        .at_state_version
        .max(highest.saturating_sub(MAX_UP_VERSION_LOOKBACK));
    if lowest > highest {
        return Ok(None);
    }

    let candidates = tx.state_version_proofs_get_range(downed.shard, lowest, highest)?;
    for candidate in candidates.into_iter().rev() {
        let commit_proof = match candidate.source {
            StateVersionProofSource::Committed { block_id } => match commit_proof_for_block(tx, &block_id) {
                Some(commit_proof) => commit_proof,
                None => continue,
            },
            StateVersionProofSource::Received { commit_proof } => commit_proof,
        };
        let Some((leaf_proof, value_hash, shard_root)) =
            up_leaf_proof(tx, downed.shard, candidate.state_version, &downed.id)
        else {
            continue;
        };
        return Ok(Some(SubstateDownProofRecord {
            state_version: candidate.state_version,
            value_hash,
            leaf_proof,
            shard_root,
            shard_root_proof: candidate.shard_root_proof,
            commit_proof,
        }));
    }
    Ok(None)
}

/// The inclusion proof of `id` in `shard`'s tree at `version`, its value hash and the shard root, or `None` if the
/// tree no longer holds that version or the substate is not in it.
fn up_leaf_proof<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    shard: Shard,
    version: Version,
    id: &VersionedSubstateId,
) -> Option<(SparseMerkleProofExt, TreeHash, TreeHash)> {
    let mut store = ShardScopedTreeStoreReader::new(tx, shard);
    let tree = SpreadPrefixStateTree::new(&mut store);
    let result = tree
        .get_proof(version, id)
        .and_then(|proof| Ok((proof, tree.get_root_hash(version)?)));
    match result {
        Ok(((_, Some((value_hash, _, _)), leaf_proof), shard_root)) => Some((leaf_proof, value_hash, shard_root)),
        Ok(((_, None, _), _)) => {
            warn!(target: LOG_TARGET, "{id} is not in {shard} at v{version}, a version it was up at");
            None
        },
        Err(e) => {
            debug!(target: LOG_TARGET, "Cannot prove {id} in {shard} at v{version}: {e}");
            None
        },
    }
}
