//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, ops::Deref};

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
    consensus_models::{
        CommittedBlockProof,
        DownedSubstate,
        StateVersionProof,
        StateVersionProofSource,
        SubstateRecord,
    },
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
/// `state_version` and the block that committed it still exist. With the commit proof it names, stored once per block
/// alongside it, it stays servable after both are pruned. The other half, the exclusion proof, is generated when it
/// is served, against the latest root.
///
/// A record holds a level-1 leaf proof and a level-2 shard-root proof; served, it carries a second level-1 and level-2
/// proof (the exclusion) and the commit proof, whose size grows with the committee's signatures.
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
    /// Level-2 proof of the shard's leaf in the state merkle root of the block `commit_proof_block` names.
    pub shard_root_proof: SparseMerkleProofExt,
    /// The block whose root commits the shard at `u`. Its CBOR-encoded `CommittedBlockProof` is stored once, under
    /// this id, for every record that cites it.
    pub commit_proof_block: BlockId,
}

impl SubstateDownProofRecord {
    /// Completes the proof with `commit_proof`, the stored commit proof of [`Self::commit_proof_block`], and `down`,
    /// the exclusion proof of the substate under the root of the commit proof the proof is served with.
    pub fn into_down_proof(self, commit_proof: Vec<u8>, down: SubstateValueProof) -> SubstateDownProof {
        SubstateDownProof {
            up: SubstateValueProof::new(
                self.shard_root,
                self.state_version,
                self.shard_root_proof,
                self.leaf_proof,
            ),
            up_value_hash: self.value_hash,
            up_commit_proof: commit_proof,
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
    commit_proof_for_block: F,
) -> Result<(), StorageError>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
    F: FnMut(&TTx::Target, &BlockId) -> Option<Vec<u8>>,
{
    let mut sources = UpVersionSources::new(commit_proof_for_block);
    for downed in downed {
        match sources.build_record(&**tx, downed)? {
            Some(record) => {
                if let Some(commit_proof) = sources.take_unstored(&record.commit_proof_block) {
                    tx.substate_down_proof_commit_proofs_insert(&record.commit_proof_block, &commit_proof)?;
                }
                tx.substate_down_proofs_insert(downed.shard, &downed.id, &record)?;
            },
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

/// The state version proofs and commit proofs an indexing pass draws on, each read or built at most once.
///
/// A block typically takes down many substates of one shard at one state version, and all of them are proved up at
/// the same newest earlier version, so one lookup of that version and one commit proof serve every one of them.
struct UpVersionSources<F> {
    commit_proof_for_block: F,
    /// `None` records a version this node holds no proof of.
    version_proofs: HashMap<(Shard, Version), Option<StateVersionProof>>,
    /// Per version, the block whose commit proof the version's proof cites, or `None` if none is available.
    anchors: HashMap<(Shard, Version), Option<BlockId>>,
    /// Commit proofs this pass resolved that are not yet stored, to be written once with the first record citing
    /// them.
    unstored: HashMap<BlockId, Vec<u8>>,
}

impl<F> UpVersionSources<F> {
    fn new(commit_proof_for_block: F) -> Self {
        Self {
            commit_proof_for_block,
            version_proofs: HashMap::new(),
            anchors: HashMap::new(),
            unstored: HashMap::new(),
        }
    }

    /// The commit proof of `block_id` if this pass resolved it and it is not yet stored, once.
    fn take_unstored(&mut self, block_id: &BlockId) -> Option<Vec<u8>> {
        self.unstored.remove(block_id)
    }

    /// The record for the greatest version below the destroying one that this node can prove the substate up at.
    ///
    /// Versions are tried newest first, one at a time, since the newest usually succeeds.
    fn build_record<TTx>(
        &mut self,
        tx: &TTx,
        downed: &DownedSubstate,
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

        for state_version in (lowest..=highest).rev() {
            let Some(candidate) = self.version_proof(tx, downed.shard, state_version)? else {
                continue;
            };
            let Some(commit_proof_block) = self.anchor(tx, downed.shard, &candidate)? else {
                continue;
            };
            let Some((leaf_proof, value_hash, shard_root)) = up_leaf_proof(tx, downed.shard, state_version, &downed.id)
            else {
                continue;
            };
            return Ok(Some(SubstateDownProofRecord {
                state_version,
                value_hash,
                leaf_proof,
                shard_root,
                shard_root_proof: candidate.shard_root_proof,
                commit_proof_block,
            }));
        }
        Ok(None)
    }

    fn version_proof<TTx: StateStoreReadTransaction>(
        &mut self,
        tx: &TTx,
        shard: Shard,
        state_version: Version,
    ) -> Result<Option<StateVersionProof>, StorageError> {
        if let Some(cached) = self.version_proofs.get(&(shard, state_version)) {
            return Ok(cached.clone());
        }
        let proof = tx
            .state_version_proofs_get_range(shard, state_version, state_version)?
            .pop();
        self.version_proofs.insert((shard, state_version), proof.clone());
        Ok(proof)
    }

    /// The block whose commit proof `candidate` cites, provided that commit proof is stored or can be.
    fn anchor<TTx>(
        &mut self,
        tx: &TTx,
        shard: Shard,
        candidate: &StateVersionProof,
    ) -> Result<Option<BlockId>, StorageError>
    where
        TTx: StateStoreReadTransaction,
        F: FnMut(&TTx, &BlockId) -> Option<Vec<u8>>,
    {
        let key = (shard, candidate.state_version);
        if let Some(anchor) = self.anchors.get(&key) {
            return Ok(*anchor);
        }
        let anchor = match &candidate.source {
            StateVersionProofSource::Committed { block_id } => {
                if self.unstored.contains_key(block_id) || tx.substate_down_proof_commit_proofs_get(block_id)?.is_some()
                {
                    Some(*block_id)
                } else {
                    (self.commit_proof_for_block)(tx, block_id).map(|commit_proof| {
                        self.unstored.insert(*block_id, commit_proof);
                        *block_id
                    })
                }
            },
            StateVersionProofSource::Received { commit_proof } => match CommittedBlockProof::from_bytes(commit_proof) {
                Ok(decoded) => {
                    let block_id = BlockId::new(decoded.block_id());
                    if !self.unstored.contains_key(&block_id) &&
                        tx.substate_down_proof_commit_proofs_get(&block_id)?.is_none()
                    {
                        self.unstored.insert(block_id, commit_proof.clone());
                    }
                    Some(block_id)
                },
                Err(e) => {
                    warn!(target: LOG_TARGET, "Held an undecodable commit proof for {shard} v{}: {e}", candidate.state_version);
                    None
                },
            },
        };
        self.anchors.insert(key, anchor);
        Ok(anchor)
    }
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
