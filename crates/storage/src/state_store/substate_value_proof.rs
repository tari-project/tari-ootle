//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::substate::{SubstateId, SubstateValue, hash_substate};
use tari_ootle_common_types::{Epoch, NumPreshards, ShardGroup, VersionedSubstateId, VotePower, shard::Shard};
use tari_sidechain::SidechainProofValidationError;
use tari_state_tree::{
    SPARSE_MERKLE_PLACEHOLDER_HASH,
    SpreadPrefixStateTree,
    SubstateValueProof,
    SubstateValueProofError,
    TreeHash,
    compute_proof_for_hashes,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    StateStoreReadTransaction,
    StorageError,
    consensus_models::{CommittedBlockProof, CommittedBlockProofError, VerifiedBlockTip},
    state_store::ShardScopedTreeStoreReader,
};

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

/// Verifies a substate value proof served by a (possibly untrusted) validator.
///
/// 1. The commit proof is validated against the shard group committee, yielding a trusted shard-group state merkle
///    root.
/// 2. The substate value proof is verified against that root - an inclusion proof for an up substate (binding the
///    returned `value` by re-deriving its leaf value hash), or an exclusion proof for a down substate (`value` is
///    `None`).
///
/// `check_vn` must return the voting power of the given validator in the committee for the commit
/// proof's epoch/shard group (zero if not a member); the caller is responsible for fetching that
/// committee (see [`CommittedBlockProof::epoch`]/[`CommittedBlockProof::shard_group`]).
#[allow(clippy::too_many_arguments)]
pub fn verify_substate_value_proof(
    commit_proof: &CommittedBlockProof,
    value_proof_bytes: &[u8],
    substate_id: &SubstateId,
    version: u32,
    value: Option<&SubstateValue>,
    proof_epoch: Epoch,
    quorum_threshold: VotePower,
    check_vn: impl Fn(&RistrettoPublicKeyBytes) -> Result<VotePower, SidechainProofValidationError>,
) -> Result<VerifiedBlockTip, SubstateProofVerifyError> {
    // Anchor: a quorum-signed shard-group state merkle root.
    let verified_tip = commit_proof.validate(quorum_threshold, check_vn)?;
    let group_root = TreeHash::new(verified_tip.state_merkle_root.into_array());

    let value_proof: SubstateValueProof = tari_bor::serde_codec::from_slice(value_proof_bytes)
        .map_err(|e| SubstateProofVerifyError::Decode(e.to_string()))?;

    let versioned_id = VersionedSubstateId::new(substate_id.clone(), version);
    match value {
        Some(value) => {
            // Bind the returned value to the committed leaf by re-deriving its value hash, so a
            // validator cannot swap the value while presenting a proof for the real committed leaf.
            let value_hash = TreeHash::new(hash_substate(value, version, proof_epoch).into_array());
            value_proof.verify_inclusion(&group_root, &versioned_id, &value_hash)?;
        },
        None => {
            value_proof.verify_exclusion(&group_root, &versioned_id)?;
        },
    }

    Ok(verified_tip)
}

#[derive(Debug, thiserror::Error)]
pub enum SubstateProofVerifyError {
    #[error("commit proof invalid: {0}")]
    CommitProof(#[from] CommittedBlockProofError),
    #[error("failed to decode substate value proof: {0}")]
    Decode(String),
    #[error("substate value proof invalid: {0}")]
    ValueProof(#[from] SubstateValueProofError),
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
