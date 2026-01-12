//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::marker::PhantomData;

use jmt::{
    storage::{TreeReader, TreeUpdateBatch},
    JellyfishMerkleTree,
    KeyHash,
    OwnedValue,
    RootHash,
    Version,
};
use log::debug;
use tari_common_types::types::FixedHash;
use tari_crypto::tari_utilities::ByteArray;
use tari_ootle_common_types::VersionedSubstateId;

use crate::{
    diff::StateHashTreeDiff,
    empty_store::UpdateBatchStore,
    error::StateTreeError,
    hasher::OotleJmtHasher,
    key_mapper::{DbKeyMapper, HashIdentityKeyMapper, SpreadPrefixKeyMapper},
    SparseMerkleProof,
    TreeStoreBatchWriter,
};

pub const SPARSE_MERKLE_PLACEHOLDER_HASH: RootHash = RootHash(*b"SPARSE_MERKLE_PLACEHOLDER_HASH__");

const LOG_TARGET: &str = "tari::ootle::state_tree";

pub type SpreadPrefixStateTree<'a, S> = StateTree<'a, S, SpreadPrefixKeyMapper>;
pub type RootStateTree<'a, S> = StateTree<'a, S, HashIdentityKeyMapper>;

pub struct StateTree<'a, S, M> {
    store: &'a mut S,
    _mapper: PhantomData<M>,
}

impl<'a, S, M> StateTree<'a, S, M> {
    pub fn new(store: &'a mut S) -> Self {
        Self {
            store,
            _mapper: PhantomData,
        }
    }
}

impl<S: TreeReader, M: DbKeyMapper<VersionedSubstateId>> StateTree<'_, S, M> {
    pub fn get_proof(
        &self,
        version: Version,
        key: &VersionedSubstateId,
    ) -> Result<(KeyHash, Option<OwnedValue>, SparseMerkleProof), StateTreeError> {
        let jmt = JellyfishMerkleTree::new(self.store);
        let key = M::map_to_leaf_key(key);
        let (maybe_value, proof) = jmt.get_with_proof(key, version)?;
        Ok((key, maybe_value, proof))
    }

    pub fn get_root_hash(&self, version: Version) -> Result<RootHash, StateTreeError> {
        let jmt = JellyfishMerkleTree::<_, OotleJmtHasher>::new(self.store);
        let root_hash = jmt.get_root_hash(version)?;
        Ok(root_hash)
    }

    fn calculate_substate_changes<I: IntoIterator<Item = SubstateTreeChange>>(
        &mut self,
        next_version: Version,
        changes: I,
    ) -> Result<(RootHash, StateHashTreeDiff), StateTreeError> {
        let (root_hash, update_batch) = calculate_substate_changes::<_, M, _>(self.store, next_version, changes)?;
        Ok((root_hash, update_batch.into()))
    }
}

impl<S: TreeReader + TreeStoreBatchWriter, M: DbKeyMapper<VersionedSubstateId>> StateTree<'_, S, M> {
    /// Stores the substate changes in the state tree and returns the new root hash.
    pub fn put_substate_changes<I: IntoIterator<Item = SubstateTreeChange>>(
        &mut self,
        next_version: Version,
        changes: I,
    ) -> Result<RootHash, StateTreeError> {
        let (root_hash, update_batch) = self.calculate_substate_changes(next_version, changes)?;
        self.commit_diff(next_version, update_batch)?;
        Ok(root_hash)
    }

    fn commit_diff(&mut self, version: Version, diff: StateHashTreeDiff) -> Result<(), StateTreeError> {
        debug!(
            target: LOG_TARGET,
            "Committing {} new nodes and {} values ({} stale tree nodes)",
            diff.new_nodes.nodes.len(),
            diff.new_nodes.values.len(),
            diff.stale_tree_nodes.len()
        );
        self.store.batch_insert_nodes(diff.new_nodes.into())?;
        self.store
            .record_stale_tree_nodes(version, diff.stale_tree_nodes.into_iter().map(Into::into).collect())?;

        Ok(())
    }
}

impl<S: TreeReader + TreeStoreBatchWriter, M: DbKeyMapper<VersionedSubstateId>> StateTree<'_, S, M> {
    /// Stores the substate changes in the state tree and returns the new root hash.
    pub fn batch_put_substate_changes<I: IntoIterator<Item = SubstateTreeChange>>(
        &mut self,
        next_version: Version,
        changes: I,
    ) -> Result<RootHash, StateTreeError> {
        let (root_hash, update_batch) = self.calculate_substate_changes(next_version, changes)?;
        log::debug!(
            target: LOG_TARGET,
            "Batch inserting {} new nodes and recording {} stale tree nodes",
            update_batch.new_nodes.nodes.len(),
            update_batch.stale_tree_nodes.len()
        );
        self.store.batch_insert_nodes(update_batch.new_nodes.into())?;
        self.store.record_stale_tree_nodes(
            next_version,
            update_batch.stale_tree_nodes.into_iter().map(Into::into).collect(),
        )?;

        Ok(root_hash)
    }
}

// impl<S: TreeReader + TreeStoreBatchWriter, M: DbKeyMapper<KeyHash>> StateTree<'_, S, M> {
//     pub fn put_changes<I: IntoIterator<Item = KeyHash>>(
//         &mut self,
//         next_version: Version,
//         changes: I,
//     ) -> Result<RootHash, StateTreeError> {
//         let (root_hash, update_result) = self.compute_update_batch(next_version, changes)?;
//
//         self.store.batch_insert_nodes(update_result.node_batch.into())?;
//         self.store.record_stale_tree_nodes(
//             next_version,
//             update_result
//                 .stale_node_index_batch
//                 .into_iter()
//                 .map(Into::into)
//                 .collect(),
//         )?;
//
//         // for (k, node) in update_result.node_batch {
//         //     self.store.insert_node(k, node)?;
//         // }
//         //
//         // for stale_tree_node in update_result.stale_node_index_batch {
//         //     self.store
//         //         .record_stale_tree_node(StaleTreeNode::Node(stale_tree_node.node_key))?;
//         // }
//
//         Ok(root_hash)
//     }
// }

impl<S: TreeReader, M: DbKeyMapper<KeyHash>> StateTree<'_, S, M> {
    pub fn compute_update_batch<I: IntoIterator<Item = KeyHash>>(
        &mut self,
        next_version: Version,
        changes: I,
    ) -> Result<(RootHash, TreeUpdateBatch), StateTreeError> {
        let jmt = JellyfishMerkleTree::<_, OotleJmtHasher>::new(self.store);

        let changes = changes
            .into_iter()
            .map(|hash| (M::map_to_leaf_key(&hash), Some(hash.0.to_vec())));

        let (root, update) = jmt.put_value_set(changes, next_version)?;
        Ok((root, update))
    }
}

/// Calculates the new root hash and tree updates for the given substate changes.
fn calculate_substate_changes<
    S: TreeReader,
    M: DbKeyMapper<VersionedSubstateId>,
    I: IntoIterator<Item = SubstateTreeChange>,
>(
    store: &S,
    next_version: Version,
    changes: I,
) -> Result<(RootHash, TreeUpdateBatch), StateTreeError> {
    let jmt = JellyfishMerkleTree::<_, OotleJmtHasher>::new(store);

    let changes = changes.into_iter().map(|ch| match ch {
        SubstateTreeChange::Up { id, value_hash } => (M::map_to_leaf_key(&id), Some(value_hash.as_slice().to_vec())),
        SubstateTreeChange::Down { id } => (M::map_to_leaf_key(&id), None),
    });

    let (root_hash, update_result) = jmt.put_value_set(changes, next_version)?;
    Ok((root_hash, update_result))
}

#[derive(Debug, Clone)]
pub enum SubstateTreeChange {
    Up {
        id: VersionedSubstateId,
        value_hash: FixedHash,
    },
    Down {
        id: VersionedSubstateId,
    },
}

impl SubstateTreeChange {
    pub fn id(&self) -> &VersionedSubstateId {
        match self {
            Self::Up { id, .. } => id,
            Self::Down { id } => id,
        }
    }
}

pub fn compute_merkle_root_for_hashes<I: IntoIterator<Item = KeyHash>>(hashes: I) -> Result<RootHash, StateTreeError> {
    let mut hashes = hashes.into_iter().peekable();
    if hashes.peek().is_none() {
        return Ok(SPARSE_MERKLE_PLACEHOLDER_HASH);
    }
    let mut store = UpdateBatchStore::empty();
    let mut root_tree = RootStateTree::new(&mut store);
    let (hash, _) = root_tree.compute_update_batch(1, hashes)?;
    Ok(hash)
}

/// Computes a Merkle proof for the given hash is either included in the provided the hashes, or proof of absence.
/// Returns the value (if it exists) and the Merkle proof.
pub fn compute_proof_for_hashes<I: Iterator<Item = KeyHash>>(
    hashes: I,
    hash_to_prove: KeyHash,
) -> Result<(Option<OwnedValue>, SparseMerkleProof), StateTreeError> {
    let mut mem_store = UpdateBatchStore::empty();
    let mut root_tree = RootStateTree::new(&mut mem_store);
    let (_, update_batch) = root_tree.compute_update_batch(1, hashes)?;
    let mem_store = UpdateBatchStore::new(Some(update_batch));
    let jmt = JellyfishMerkleTree::new(&mem_store);
    let proof_tuple = jmt.get_with_proof(hash_to_prove, 1)?;
    Ok(proof_tuple)
}
