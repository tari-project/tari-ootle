//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::marker::PhantomData;

use serde::{Deserialize, Serialize};
use tari_jellyfish::{
    JellyfishMerkleTree,
    LeafKey,
    Node,
    NodeKey,
    ProofValue,
    SparseMerkleProofExt,
    StaleTreeNode,
    TreeHash,
    TreeStore,
    TreeStoreReader,
    TreeStoreWriter,
    TreeUpdateBatch,
    Version,
};
use tari_ootle_common_types::{ToSubstateAddress, VersionedSubstateId, shard::Shard};
use tari_template_lib_types::Hash32;

use crate::{
    SPARSE_MERKLE_PLACEHOLDER_HASH,
    StateTreePayload,
    TreeStoreBatchWriter,
    error::StateTreeError,
    key_mapper::{DbKeyMapper, HashIdentityKeyMapper, ShardKeyMapper, SpreadPrefixKeyMapper},
    memory_store::MemoryTreeStore,
};

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

impl<S: TreeStoreReader<StateTreePayload>, M: DbKeyMapper<VersionedSubstateId>> StateTree<'_, S, M> {
    pub fn get_proof(
        &self,
        version: Version,
        key: &VersionedSubstateId,
    ) -> Result<(LeafKey, Option<ProofValue<StateTreePayload>>, SparseMerkleProofExt), StateTreeError> {
        let jmt = JellyfishMerkleTree::new(self.store);
        let key = M::map_to_leaf_key(key);
        let (maybe_value, proof) = jmt.get_with_proof_ext(key.as_ref(), version)?;
        Ok((key, maybe_value, proof))
    }

    pub fn get_root_hash(&self, version: Version) -> Result<TreeHash, StateTreeError> {
        let jmt = JellyfishMerkleTree::new(self.store);
        let root_hash = jmt.get_root_hash(version)?;
        Ok(root_hash)
    }

    fn calculate_substate_changes<I: IntoIterator<Item = SubstateTreeChange>>(
        &mut self,
        current_version: Option<Version>,
        next_version: Version,
        changes: I,
    ) -> Result<(TreeHash, StateHashTreeDiff<StateTreePayload>), StateTreeError> {
        let (root_hash, update_batch) =
            calculate_substate_changes::<_, M, _>(self.store, current_version, next_version, changes)?;
        Ok((root_hash, update_batch.into()))
    }
}

impl<S: TreeStore<StateTreePayload>, M: DbKeyMapper<VersionedSubstateId>> StateTree<'_, S, M> {
    /// Stores the substate changes in the state tree and returns the new root hash.
    pub fn put_substate_changes<I: IntoIterator<Item = SubstateTreeChange>>(
        &mut self,
        current_version: Option<Version>,
        next_version: Version,
        changes: I,
    ) -> Result<TreeHash, StateTreeError> {
        let (root_hash, update_batch) = self.calculate_substate_changes(current_version, next_version, changes)?;
        self.commit_diff(update_batch)?;
        Ok(root_hash)
    }

    fn commit_diff(&mut self, diff: StateHashTreeDiff<StateTreePayload>) -> Result<(), StateTreeError> {
        for (key, node) in diff.new_nodes {
            log::debug!("Inserting node: {}", key);
            self.store.insert_node(key, node)?;
        }

        for stale_tree_node in diff.stale_tree_nodes {
            log::debug!("Recording stale tree node: {}", stale_tree_node.as_node_key());
            self.store.record_stale_tree_node(stale_tree_node)?;
        }

        Ok(())
    }
}

impl<S: TreeStoreReader<StateTreePayload> + TreeStoreBatchWriter<StateTreePayload>, M: DbKeyMapper<VersionedSubstateId>>
    StateTree<'_, S, M>
{
    /// Stores the substate changes in the state tree and returns the new root hash.
    pub fn batch_put_substate_changes<I: IntoIterator<Item = SubstateTreeChange>>(
        &mut self,
        current_version: Option<Version>,
        next_version: Version,
        changes: I,
    ) -> Result<TreeHash, StateTreeError> {
        let (root_hash, update_batch) = self.calculate_substate_changes(current_version, next_version, changes)?;
        log::debug!(
            target: LOG_TARGET,
            "Batch inserting {} new nodes and recording {} stale tree nodes",
            update_batch.new_nodes.len(),
            update_batch.stale_tree_nodes.len()
        );
        self.store.batch_insert_nodes(update_batch.new_nodes)?;
        self.store
            .record_stale_tree_nodes(next_version, update_batch.stale_tree_nodes)?;

        Ok(root_hash)
    }
}

impl<S: TreeStore<()>, M: DbKeyMapper<TreeHash>> StateTree<'_, S, M> {
    pub fn put_changes<I: IntoIterator<Item = TreeHash>>(
        &mut self,
        current_version: Option<Version>,
        next_version: Version,
        changes: I,
    ) -> Result<TreeHash, StateTreeError> {
        let (root_hash, update_result) = self.compute_update_batch(current_version, next_version, changes)?;

        for (k, node) in update_result.node_batch {
            self.store.insert_node(k, node)?;
        }

        for stale_tree_node in update_result.stale_node_index_batch {
            self.store
                .record_stale_tree_node(StaleTreeNode::Node(stale_tree_node.node_key))?;
        }

        Ok(root_hash)
    }

    pub fn compute_update_batch<I: IntoIterator<Item = TreeHash>>(
        &mut self,
        current_version: Option<Version>,
        next_version: Version,
        changes: I,
    ) -> Result<(TreeHash, TreeUpdateBatch<()>), StateTreeError> {
        let jmt = JellyfishMerkleTree::<_, ()>::new(self.store);

        let changes = changes
            .into_iter()
            .map(|hash| (M::map_to_leaf_key(&hash), Some((hash, ()))));

        let (root, update) = jmt.batch_put_value_set(changes, None, current_version, next_version)?;
        Ok((root, update))
    }
}

/// Calculates the new root hash and tree updates for the given substate changes.
fn calculate_substate_changes<
    S: TreeStoreReader<StateTreePayload>,
    M: DbKeyMapper<VersionedSubstateId>,
    I: IntoIterator<Item = SubstateTreeChange>,
>(
    store: &mut S,
    current_version: Option<Version>,
    next_version: Version,
    changes: I,
) -> Result<(TreeHash, TreeUpdateBatch<StateTreePayload>), StateTreeError> {
    // JMT nodes are keyed by (version, nibble_path). Writing a version that is not strictly ahead of
    // the base version overwrites live nodes with keys that this same write records as stale, so the
    // stale-node GC later deletes nodes the current tree still points at. Callers that stage changes
    // and write them later are additionally guarded at the write funnel, in
    // `ShardScopedTreeStoreWriter::set_state_version`.
    if let Some(current_version) = current_version &&
        next_version <= current_version
    {
        return Err(StateTreeError::NonMonotonicVersion {
            current_version,
            next_version,
        });
    }

    let jmt = JellyfishMerkleTree::new(store);

    let changes = changes.into_iter().map(|ch| match ch {
        SubstateTreeChange::Up { id, value_hash } => (
            M::map_to_leaf_key(&id),
            Some((TreeHash::new(value_hash.into_array()), id.to_substate_address())),
        ),
        SubstateTreeChange::Down { id } => (M::map_to_leaf_key(&id), None),
    });

    let (root_hash, update_result) = jmt.batch_put_value_set(changes, None, current_version, next_version)?;

    Ok((root_hash, update_result))
}

pub enum SubstateTreeChange {
    Up {
        id: VersionedSubstateId,
        value_hash: Hash32,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StateHashTreeDiff<P> {
    pub new_nodes: Vec<(NodeKey, Node<P>)>,
    pub stale_tree_nodes: Vec<StaleTreeNode>,
}

// NodeKey, Node and StaleTreeNode come from tari_jellyfish (external git dep) and only implement
// serde. Bridge the whole struct through `tari_bor::adapters::serde_bridge` rather than forking
// the upstream crate. minicbor's derive doesn't accept where-bounds on generics, hence the
// manual impls.
impl<C, P> minicbor::Encode<C> for StateHashTreeDiff<P>
where P: serde::Serialize
{
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        tari_bor::adapters::serde_bridge::encode(self, e, ctx)
    }
}

impl<'b, C, P> minicbor::Decode<'b, C> for StateHashTreeDiff<P>
where P: serde::Deserialize<'b>
{
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        tari_bor::adapters::serde_bridge::decode(d, ctx)
    }
}

impl<C, P> minicbor::CborLen<C> for StateHashTreeDiff<P>
where P: serde::Serialize
{
    fn cbor_len(&self, ctx: &mut C) -> usize {
        tari_bor::adapters::serde_bridge::cbor_len(self, ctx)
    }
}

impl<P> StateHashTreeDiff<P> {
    pub fn new() -> Self {
        Self {
            new_nodes: Vec::new(),
            stale_tree_nodes: Vec::new(),
        }
    }
}

impl<P> From<TreeUpdateBatch<P>> for StateHashTreeDiff<P> {
    fn from(batch: TreeUpdateBatch<P>) -> Self {
        Self {
            new_nodes: batch.node_batch,
            stale_tree_nodes: batch
                .stale_node_index_batch
                .into_iter()
                .map(|node| StaleTreeNode::Node(node.node_key))
                .collect(),
        }
    }
}

pub fn compute_merkle_root_for_hashes<I: IntoIterator<Item = TreeHash>>(hashes: I) -> Result<TreeHash, StateTreeError> {
    let mut hashes = hashes.into_iter().peekable();
    if hashes.peek().is_none() {
        return Ok(SPARSE_MERKLE_PLACEHOLDER_HASH);
    }
    let mut mem_store = MemoryTreeStore::new();
    let mut root_tree = RootStateTree::new(&mut mem_store);
    let (hash, _) = root_tree.compute_update_batch(None, 1, hashes)?;
    Ok(hash)
}

/// An ephemeral tree over a set of hashes, held so that several of them can be proved against the
/// same root without rebuilding it.
///
/// Building costs O(n log n) hashes in n, the size of the set; each proof after that is one
/// traversal. Callers proving more than one hash against the same set should build this once -
/// [`compute_proof_for_hashes`] is the one-shot form and rebuilds the tree per proof.
pub struct RootProofTree {
    store: MemoryTreeStore<()>,
}

impl RootProofTree {
    pub fn build<I: IntoIterator<Item = TreeHash>>(hashes: I) -> Result<Self, StateTreeError> {
        let mut store = MemoryTreeStore::new();
        RootStateTree::new(&mut store).put_changes(None, 1, hashes)?;
        Ok(Self { store })
    }

    /// Proves that `hash_to_prove` is one of the hashes the tree was built over, or that it is not.
    /// Returns the value (if it exists) and the Merkle proof.
    pub fn get_proof(
        &self,
        hash_to_prove: TreeHash,
    ) -> Result<(Option<ProofValue<()>>, SparseMerkleProofExt), StateTreeError> {
        let jmt = JellyfishMerkleTree::new(&self.store);
        let key = HashIdentityKeyMapper::map_to_leaf_key(&hash_to_prove);
        let proof_tuple = jmt.get_with_proof_ext(key.as_ref(), 1)?;
        Ok(proof_tuple)
    }
}

/// The shard-group state root tree: an ephemeral tree over the shard group's per-shard JMT roots,
/// with each root stored at a leaf keyed by its shard. Its root is the `state_merkle_root` a block
/// header commits.
///
/// Keying by shard is what lets a proof name the shard a root belongs to. A verifier proves a
/// substate against the root at the leaf of the substate's own shard, so no other shard's root can
/// stand in for it.
///
/// A shard whose JMT is empty has no leaf: the absence of its leaf commits to the empty-tree root
/// [`SPARSE_MERKLE_PLACEHOLDER_HASH`]. This keeps the tree, and the cost of building it for every
/// block, proportional to the shards that hold state.
pub struct ShardGroupRootTree {
    store: MemoryTreeStore<()>,
    root: TreeHash,
}

impl ShardGroupRootTree {
    /// Builds the tree over `(shard, root)` pairs. Each shard must appear at most once.
    pub fn build<I: IntoIterator<Item = (Shard, TreeHash)>>(shard_roots: I) -> Result<Self, StateTreeError> {
        let mut store = MemoryTreeStore::<()>::new();
        let mut changes = shard_roots
            .into_iter()
            .filter(|(_, root)| *root != SPARSE_MERKLE_PLACEHOLDER_HASH)
            .map(|(shard, root)| (ShardKeyMapper::map_to_leaf_key(&shard), Some((root, ()))))
            .peekable();
        if changes.peek().is_none() {
            return Ok(Self {
                store,
                root: SPARSE_MERKLE_PLACEHOLDER_HASH,
            });
        }
        let (root, update) = JellyfishMerkleTree::<_, ()>::new(&store).batch_put_value_set(
            changes,
            None,
            None,
            SHARD_GROUP_ROOT_VERSION,
        )?;
        for (key, node) in update.node_batch {
            store.insert_node(key, node)?;
        }
        Ok(Self { store, root })
    }

    pub fn root(&self) -> TreeHash {
        self.root
    }

    /// Proves the root stored for `shard`, or that the tree holds none for it - which is the proof
    /// that the shard's root is the empty-tree root.
    pub fn get_proof(&self, shard: Shard) -> Result<(Option<ProofValue<()>>, SparseMerkleProofExt), StateTreeError> {
        let jmt = JellyfishMerkleTree::new(&self.store);
        let key = ShardKeyMapper::map_to_leaf_key(&shard);
        let proof_tuple = jmt.get_with_proof_ext(key.as_ref(), SHARD_GROUP_ROOT_VERSION)?;
        Ok(proof_tuple)
    }
}

const SHARD_GROUP_ROOT_VERSION: Version = 1;

/// Computes the shard-group state root over `(shard, root)` pairs. See [`ShardGroupRootTree`].
pub fn compute_shard_group_root<I: IntoIterator<Item = (Shard, TreeHash)>>(
    shard_roots: I,
) -> Result<TreeHash, StateTreeError> {
    Ok(ShardGroupRootTree::build(shard_roots)?.root())
}

/// Computes a Merkle proof for the given hash is either included in the provided the hashes, or proof of absence.
/// Returns the value (if it exists) and the Merkle proof.
pub fn compute_proof_for_hashes<I: Iterator<Item = TreeHash>>(
    hashes: I,
    hash_to_prove: TreeHash,
) -> Result<(Option<ProofValue<()>>, SparseMerkleProofExt), StateTreeError> {
    RootProofTree::build(hashes)?.get_proof(hash_to_prove)
}
