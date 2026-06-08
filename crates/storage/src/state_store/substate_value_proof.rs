//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{NumPreshards, ShardGroup, VersionedSubstateId, shard::Shard};
use tari_state_tree::{
    SPARSE_MERKLE_PLACEHOLDER_HASH,
    SpreadPrefixStateTree,
    SubstateValueProof,
    TreeHash,
    compute_proof_for_hashes,
};

use crate::{StateStoreReadTransaction, StorageError, state_store::ShardScopedTreeStoreReader};

/// Generates a two-level [`SubstateValueProof`] for `versioned_id` against the latest committed
/// shard-group state.
///
/// The proof is rooted at the shard-group `state_merkle_root` - the same root the latest committed
/// block header commits - so a caller that independently trusts that root (e.g. via a verified
/// committed block proof) can verify the substate's committed value or its absence without trusting
/// the node that produced the proof.
///
/// Whether the leaf proof is an inclusion or exclusion proof depends on whether `versioned_id` is
/// currently up in its shard; the caller chooses `verify_inclusion`/`verify_exclusion` accordingly.
pub fn generate_substate_proof<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    shard_group: ShardGroup,
    versioned_id: &VersionedSubstateId,
    num_preshards: NumPreshards,
) -> Result<SubstateValueProof, StorageError> {
    // Per-shard roots in the canonical order the block header commits them: [global, shard_0, ...].
    let mut ordered_roots = Vec::with_capacity(shard_group.len() + 1);
    ordered_roots.push(shard_state_root(tx, Shard::global())?);
    for shard in shard_group.shard_iter() {
        ordered_roots.push(shard_state_root(tx, shard)?);
    }

    // Level 1: leaf proof (inclusion or exclusion) for the substate within its shard.
    let shard = versioned_id.to_shard(num_preshards);
    let version = tx
        .state_tree_versions_get_latest(shard)?
        .ok_or_else(|| StorageError::QueryError {
            reason: format!("generate_substate_proof: shard {shard} has no committed state"),
        })?;
    let mut scoped = ShardScopedTreeStoreReader::new(tx, shard);
    let tree = SpreadPrefixStateTree::new(&mut scoped);
    let (_leaf_key, _proof_value, leaf_proof) =
        tree.get_proof(version, versioned_id)
            .map_err(|e| StorageError::QueryError {
                reason: format!("generate_substate_proof get_proof: {e}"),
            })?;
    let shard_root = tree.get_root_hash(version).map_err(|e| StorageError::QueryError {
        reason: format!("generate_substate_proof get_root_hash: {e}"),
    })?;

    // Level 2: prove the shard root is committed in the shard-group root.
    let (_, shard_root_proof) =
        compute_proof_for_hashes(ordered_roots.into_iter(), shard_root).map_err(|e| StorageError::QueryError {
            reason: format!("generate_substate_proof shard root proof: {e}"),
        })?;

    Ok(SubstateValueProof::new(shard_root, shard_root_proof, leaf_proof))
}

/// The committed JMT root of `shard`, or the empty-tree placeholder if the shard has no state yet.
fn shard_state_root<TTx: StateStoreReadTransaction>(tx: &TTx, shard: Shard) -> Result<TreeHash, StorageError> {
    let Some(version) = tx.state_tree_versions_get_latest(shard)? else {
        return Ok(SPARSE_MERKLE_PLACEHOLDER_HASH);
    };
    let mut scoped = ShardScopedTreeStoreReader::new(tx, shard);
    let tree = SpreadPrefixStateTree::new(&mut scoped);
    tree.get_root_hash(version).map_err(|e| StorageError::QueryError {
        reason: format!("generate_substate_proof shard {shard} root: {e}"),
    })
}
