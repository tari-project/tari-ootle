//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Checks a shard snapshot, the substates a shard's tree holds at a verified version, against the shard root its
//! verified leaf commits.
//!
//! The indexer keys and hashes each leaf itself from the substate the peer sends, so a substate added, left out,
//! altered or misplaced in the order builds another root.

use ootle_network::Network;
use prost::Message;
use tari_engine_types::{limits::MAX_CBOR_NESTING_DEPTH, substate::SubstateId};
use tari_ootle_common_types::Epoch;
use tari_ootle_p2p::proto::rpc;
use tari_ootle_storage::consensus_models::{
    SubstateCreate,
    SubstateData,
    SubstateUpdateProof,
    SubstateValueFilterFlags,
};
use tari_state_tree::{
    JmtHashScheme,
    LeafKey,
    SortedLeafRootBuilder,
    SparseMerkleProofExt,
    SubstateTreeChange,
    TreeHash,
    key_mapper::{DbKeyMapper, SpreadPrefixKeyMapper},
};

use crate::{
    network_state_sync::{validator_status::ProvedShardLeaf, verified_stage::streamed_tree_change},
    storage_sqlite::models::SnapshotSubstate,
};

/// A shard snapshot as it is paged in, checked against the leaf it was taken at.
pub struct ShardSnapshot {
    network: Network,
    leaf: ProvedShardLeaf,
    value_filters: SubstateValueFilterFlags,
    root: SortedLeafRootBuilder,
}

impl ShardSnapshot {
    /// A snapshot of `leaf`'s shard at its version, whose substates `value_filters` selects are indexed.
    pub fn new(network: Network, leaf: ProvedShardLeaf, value_filters: SubstateValueFilterFlags) -> Self {
        Self {
            network,
            leaf,
            value_filters,
            root: SortedLeafRootBuilder::new(JmtHashScheme::V1),
        }
    }

    /// The leaf key of the last substate added, which the next page starts above.
    pub fn last_key(&self) -> Option<&LeafKey> {
        self.root.last_key()
    }

    /// The number of substates added.
    pub fn num_substates(&self) -> usize {
        self.root.num_leaves()
    }

    /// Adds a page of the snapshot, which must continue in leaf key order from the last substate added, and checks it
    /// against the leaf's shard root through `last_leaf_proof`, the proof of its last substate's leaf. Returns the
    /// substates to stage: those the value filters select. The others index nothing.
    ///
    /// The check covers every substate added so far, so a page that leaves out, adds or alters one fails as it
    /// arrives. A selected substate other than a template must carry its value, which the snapshot indexes it from.
    pub fn add_page(
        &mut self,
        substates: Vec<rpc::SnapshotSubstate>,
        last_leaf_proof: &[u8],
    ) -> Result<Vec<SnapshotSubstate>, SnapshotError> {
        if substates.is_empty() {
            return Ok(Vec::new());
        }
        let mut staged = Vec::new();
        for substate in substates {
            let created_epoch = substate
                .created_epoch
                .map(Epoch::from)
                .ok_or_else(|| SnapshotError::Malformed("a substate has no created epoch".to_string()))?;
            let encoded = substate
                .substate
                .ok_or_else(|| SnapshotError::Malformed("a substate has no data".to_string()))?;
            let data = SubstateData::try_from(encoded).map_err(|e| SnapshotError::Malformed(e.to_string()))?;
            let substate_id = data.substate_id.clone();
            let is_template = substate_id.is_template();
            if data.template_metadata.is_some() && !is_template {
                return Err(SnapshotError::Malformed(format!(
                    "{substate_id} carries template metadata, though it is not a template"
                )));
            }
            let is_indexed = self.value_filters.contains_substate(&substate_id);
            let carries_value = data.value.value().is_some() || (is_template && data.template_metadata.is_some());
            if is_indexed && !carries_value {
                return Err(SnapshotError::ValueWithheld { substate_id });
            }
            let staged_substate = is_indexed.then(|| rpc::SubstateData::from(data.clone()).encode_to_vec());
            let update = SubstateUpdateProof::Create(Box::new(SubstateCreate { substate: data }));
            let SubstateTreeChange::Up { id, value_hash } = streamed_tree_change(self.network, created_epoch, &update)
                .map_err(|e| SnapshotError::Malformed(e.to_string()))?
            else {
                unreachable!("a created substate changes the tree with an up");
            };
            let leaf_key = SpreadPrefixKeyMapper::map_to_leaf_key(&id);
            self.root
                .push(leaf_key, &TreeHash::new(value_hash.into_array()))
                .map_err(|e| SnapshotError::Malformed(e.to_string()))?;
            if let Some(substate) = staged_substate {
                staged.push(SnapshotSubstate {
                    epoch: created_epoch,
                    leaf_key,
                    substate,
                });
            }
        }
        let proof: SparseMerkleProofExt =
            tari_bor::serde_codec::from_slice_with_max_depth(last_leaf_proof, MAX_CBOR_NESTING_DEPTH)
                .map_err(|e| SnapshotError::Malformed(format!("undecodable last leaf proof: {e}")))?;
        let built = self
            .root
            .root_through_last(proof.into_compact(JmtHashScheme::V1).siblings())
            .map_err(|e| SnapshotError::Malformed(e.to_string()))?;
        if built != self.leaf.shard_root {
            return Err(SnapshotError::RootMismatch {
                num_substates: self.root.num_leaves(),
                built,
                committed: self.leaf.shard_root,
            });
        }
        Ok(staged)
    }

    /// Checks that the substates added rebuild the root the snapshot's leaf commits.
    pub fn finish(self) -> Result<ProvedShardLeaf, SnapshotError> {
        let num_substates = self.root.num_leaves();
        let built = self.root.finish();
        if built != self.leaf.shard_root {
            return Err(SnapshotError::RootMismatch {
                num_substates,
                built,
                committed: self.leaf.shard_root,
            });
        }
        Ok(self.leaf)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("the snapshot is malformed: {0}")]
    Malformed(String),
    #[error("the snapshot sends {substate_id} as its hash alone, though the value filters select it")]
    ValueWithheld { substate_id: SubstateId },
    #[error("the snapshot's {num_substates} substates build root {built}, but the verified leaf commits {committed}")]
    RootMismatch {
        num_substates: usize,
        built: TreeHash,
        committed: TreeHash,
    },
}

#[cfg(test)]
mod tests {
    use tari_engine_types::{
        non_fungible::NonFungibleContainer,
        published_template::{PublishedTemplate, PublishedTemplateAddress},
        substate::{SubstateId, SubstateValue, hash_substate},
    };
    use tari_ootle_common_types::{StateVersion, SubstateVersion, VersionedSubstateId, shard::Shard};
    use tari_ootle_storage::consensus_models::SubstateValueOrHash;
    use tari_state_tree::{JellyfishMerkleTree, TransitionAccumulator, TreeStoreWriter, memory_store::MemoryTreeStore};
    use tari_template_lib_types::{
        ComponentAddress,
        Hash32,
        ObjectKey,
        ResourceAddress,
        crypto::RistrettoPublicKeyBytes,
    };

    use super::*;
    use crate::network_state_sync::verified_stage::VerifiedShardLeaf;

    const NETWORK: Network = Network::LocalNet;

    fn shard() -> Shard {
        Shard::from_u32(3)
    }

    fn component(seed: u8) -> (SubstateId, SubstateValue) {
        let id = SubstateId::Component(ComponentAddress::new(ObjectKey::from_array([seed; ObjectKey::LENGTH])));
        (id, SubstateValue::NonFungible(NonFungibleContainer::no_data()))
    }

    fn template(seed: u8) -> (SubstateId, SubstateValue) {
        let template = PublishedTemplate {
            template_name: "snapshot".try_into().unwrap(),
            author: RistrettoPublicKeyBytes::default(),
            binary: vec![seed; 16].try_into().unwrap(),
            at_epoch: 1,
            metadata_hash: None,
        };
        let address =
            PublishedTemplateAddress::from_author_and_binary_hash(&template.author, &template.to_binary_hash());
        (SubstateId::Template(address), SubstateValue::Template(template))
    }

    /// A substate as the responder serves it under `value_filters`, created at `epoch`.
    fn served(
        (id, value): (SubstateId, SubstateValue),
        epoch: Epoch,
        value_filters: SubstateValueFilterFlags,
    ) -> rpc::SnapshotSubstate {
        let version = SubstateVersion::ZERO;
        let state_hash = hash_substate(NETWORK, &value, version, epoch);
        let is_template = id.is_template();
        let template_metadata = (is_template && value_filters.contains(SubstateValueFilterFlags::TEMPLATE_METADATA))
            .then(|| value.as_template().unwrap().clone().into_template_metadata());
        let value = if value_filters.contains_substate(&id) && !is_template {
            SubstateValueOrHash::Value(Box::new(value))
        } else {
            SubstateValueOrHash::Hash(state_hash)
        };
        rpc::SnapshotSubstate {
            substate: Some(
                SubstateData {
                    substate_id: id,
                    version,
                    value,
                    template_metadata,
                }
                .into(),
            ),
            created_epoch: Some(epoch.into()),
        }
    }

    fn leaf_key_of(substate: &rpc::SnapshotSubstate) -> LeafKey {
        let data = SubstateData::try_from(substate.substate.clone().unwrap()).unwrap();
        SpreadPrefixKeyMapper::map_to_leaf_key(&VersionedSubstateId::new(data.substate_id, data.version))
    }

    fn resource(seed: u8) -> (SubstateId, SubstateValue) {
        let id = SubstateId::Resource(ResourceAddress::new(ObjectKey::from_array([seed; ObjectKey::LENGTH])));
        (id, SubstateValue::NonFungible(NonFungibleContainer::no_data()))
    }

    /// A shard's substates in leaf key order, as served under [`filters`], and the tree holding them.
    struct ShardState {
        substates: Vec<rpc::SnapshotSubstate>,
        root: TreeHash,
        tree: MemoryTreeStore<()>,
    }

    impl ShardState {
        fn new() -> Self {
            let mut substates = (0..20u8)
                .map(|seed| served(component(seed), Epoch(u64::from(seed % 3) + 1), filters()))
                .chain((0..3u8).map(|seed| served(template(seed), Epoch(2), filters())))
                .chain((0..5u8).map(|seed| served(resource(seed), Epoch(1), filters())))
                .collect::<Vec<_>>();
            substates.sort_by_key(leaf_key_of);

            let mut tree = MemoryTreeStore::new();
            let values = substates
                .iter()
                .map(|substate| (leaf_key_of(substate), Some((value_hash_of(substate), ()))));
            let (root, diff) = JellyfishMerkleTree::new(&tree, JmtHashScheme::V1)
                .batch_put_value_set(values, None, 1)
                .unwrap();
            for (key, node) in diff.node_batch {
                tree.insert_node(key, node).unwrap();
            }
            Self { substates, root, tree }
        }

        /// The encoded proof of `key`'s leaf in the tree, as a page that ends with it carries.
        fn proof(&self, key: &LeafKey) -> Vec<u8> {
            let (_, proof) = JellyfishMerkleTree::new(&self.tree, JmtHashScheme::V1)
                .get_with_proof_ext(key.as_ref(), 1)
                .unwrap();
            tari_bor::serde_codec::to_vec(&proof).unwrap()
        }

        /// Adds `substates` to `snapshot` in pages of `page_size`, each carrying the tree's proof of its last leaf.
        fn add_pages(
            &self,
            snapshot: &mut ShardSnapshot,
            substates: &[rpc::SnapshotSubstate],
            page_size: usize,
        ) -> Result<Vec<SnapshotSubstate>, SnapshotError> {
            let mut staged = Vec::new();
            for page in substates.chunks(page_size) {
                let proof = self.proof(&leaf_key_of(page.last().unwrap()));
                staged.extend(snapshot.add_page(page.to_vec(), &proof)?);
            }
            Ok(staged)
        }

        fn snapshot(&self) -> ShardSnapshot {
            ShardSnapshot::new(NETWORK, leaf_with_root(self.root), filters())
        }
    }

    fn value_hash_of(substate: &rpc::SnapshotSubstate) -> TreeHash {
        let data = SubstateData::try_from(substate.substate.clone().unwrap()).unwrap();
        let epoch = Epoch::from(substate.created_epoch.unwrap());
        let value_hash = match &data.template_metadata {
            Some(metadata) => {
                tari_engine_types::substate::hash_published_template_metadata(NETWORK, metadata, data.version, epoch)
            },
            None => data.to_value_hash(NETWORK, epoch),
        };
        TreeHash::new(value_hash.into_array())
    }

    fn leaf_with_root(shard_root: TreeHash) -> ProvedShardLeaf {
        ProvedShardLeaf {
            leaf: VerifiedShardLeaf {
                shard: shard(),
                state_version: StateVersion::new(9),
                transition_acc: TransitionAccumulator::new([9; 32]),
            },
            shard_root,
        }
    }

    fn filters() -> SubstateValueFilterFlags {
        SubstateValueFilterFlags::COMPONENT | SubstateValueFilterFlags::TEMPLATE_METADATA
    }

    #[test]
    fn a_snapshot_paged_in_order_rebuilds_the_committed_root() {
        let state = ShardState::new();
        for page_size in [1, 4, 28, 100] {
            let mut snapshot = state.snapshot();
            let staged = state.add_pages(&mut snapshot, &state.substates, page_size).unwrap();
            assert_eq!(snapshot.last_key(), Some(&leaf_key_of(state.substates.last().unwrap())));
            assert_eq!(snapshot.num_substates(), 28);
            assert_eq!(snapshot.finish().unwrap(), leaf_with_root(state.root));
            // The components and templates the filters select are staged, the resources are not
            assert_eq!(staged.len(), 23, "page size {page_size}");
        }
    }

    #[test]
    fn a_page_missing_altering_or_adding_a_substate_fails_as_it_arrives() {
        let state = ShardState::new();
        let first_page_fails = |substates: Vec<rpc::SnapshotSubstate>| {
            let mut snapshot = state.snapshot();
            let proof = state.proof(&leaf_key_of(&substates[9]));
            matches!(
                snapshot.add_page(substates[..10].to_vec(), &proof),
                Err(SnapshotError::RootMismatch { .. } | SnapshotError::Malformed(_))
            )
        };

        let mut missing = state.substates.clone();
        missing.remove(4);
        assert!(first_page_fails(missing));

        let mut altered = state.substates.clone();
        let index = altered
            .iter()
            .position(|substate| {
                let data = SubstateData::try_from(substate.substate.clone().unwrap()).unwrap();
                data.value.value().is_some()
            })
            .unwrap();
        altered[index].created_epoch = Some(Epoch(99).into());
        assert!(first_page_fails(altered));

        let added = served(component(200), Epoch(1), filters());
        let mut extra = state.substates.clone();
        extra.push(added.clone());
        extra.sort_by_key(leaf_key_of);
        let position = extra.iter().position(|substate| *substate == added).unwrap();
        // The page ends at a substate the tree holds, so its proof is an inclusion proof
        let end = if position + 1 < extra.len() {
            position + 1
        } else {
            position - 1
        };
        let mut snapshot = state.snapshot();
        let proof = state.proof(&leaf_key_of(&extra[end]));
        assert!(matches!(
            snapshot.add_page(extra[..=end].to_vec(), &proof),
            Err(SnapshotError::RootMismatch { .. } | SnapshotError::Malformed(_))
        ));
    }

    #[test]
    fn a_page_proved_by_another_leaf_does_not_match() {
        let state = ShardState::new();
        let mut snapshot = state.snapshot();
        let proof = state.proof(&leaf_key_of(&state.substates[10]));
        assert!(matches!(
            snapshot.add_page(state.substates[..10].to_vec(), &proof),
            Err(SnapshotError::RootMismatch { .. } | SnapshotError::Malformed(_))
        ));
        let mut snapshot = state.snapshot();
        assert!(matches!(
            snapshot.add_page(state.substates[..10].to_vec(), &[0xff]),
            Err(SnapshotError::Malformed(_))
        ));
    }

    #[test]
    fn a_snapshot_cut_short_does_not_match() {
        let state = ShardState::new();
        let mut snapshot = state.snapshot();
        state.add_pages(&mut snapshot, &state.substates[..20], 10).unwrap();
        assert!(matches!(snapshot.finish(), Err(SnapshotError::RootMismatch { .. })));
    }

    #[test]
    fn a_selected_substate_sent_as_its_hash_is_withheld() {
        let state = ShardState::new();
        let mut withheld = state.substates.clone();
        let index = withheld
            .iter()
            .position(|substate| {
                let data = SubstateData::try_from(substate.substate.clone().unwrap()).unwrap();
                data.value.value().is_some()
            })
            .unwrap();
        let mut data = SubstateData::try_from(withheld[index].substate.take().unwrap()).unwrap();
        let epoch = Epoch::from(withheld[index].created_epoch.unwrap());
        data.value = SubstateValueOrHash::Hash(data.to_value_hash(NETWORK, epoch));
        withheld[index].substate = Some(data.into());
        let mut snapshot = state.snapshot();
        assert!(matches!(
            state.add_pages(&mut snapshot, &withheld, 100),
            Err(SnapshotError::ValueWithheld { .. })
        ));
    }

    #[test]
    fn template_metadata_on_another_substate_is_malformed() {
        let state = ShardState::new();
        let template_metadata = state
            .substates
            .iter()
            .find_map(|substate| {
                let data = SubstateData::try_from(substate.substate.clone().unwrap()).unwrap();
                data.template_metadata
            })
            .unwrap();
        let mut disguised = state.substates.clone();
        let index = disguised
            .iter()
            .position(|substate| {
                let data = SubstateData::try_from(substate.substate.clone().unwrap()).unwrap();
                data.value.value().is_some()
            })
            .unwrap();
        let mut data = SubstateData::try_from(disguised[index].substate.take().unwrap()).unwrap();
        let epoch = Epoch::from(disguised[index].created_epoch.unwrap());
        data.value = SubstateValueOrHash::Hash(data.to_value_hash(NETWORK, epoch));
        data.template_metadata = Some(template_metadata);
        disguised[index].substate = Some(data.into());
        let mut snapshot = state.snapshot();
        assert!(matches!(
            state.add_pages(&mut snapshot, &disguised, 100),
            Err(SnapshotError::Malformed(_))
        ));
    }

    #[test]
    fn a_snapshot_out_of_key_order_is_malformed() {
        let state = ShardState::new();
        let mut swapped = state.substates.clone();
        swapped.swap(3, 4);
        let mut snapshot = state.snapshot();
        assert!(matches!(
            state.add_pages(&mut snapshot, &swapped, 100),
            Err(SnapshotError::Malformed(_))
        ));

        let mut snapshot = state.snapshot();
        state.add_pages(&mut snapshot, &state.substates[..10], 100).unwrap();
        assert!(matches!(
            state.add_pages(&mut snapshot, &state.substates[5..10], 100),
            Err(SnapshotError::Malformed(_))
        ));
        // A page that skips ahead leaves out the substates between, which its proof catches
        let mut snapshot = state.snapshot();
        assert!(matches!(
            state.add_pages(&mut snapshot, &state.substates[5..], 100),
            Err(SnapshotError::RootMismatch { .. })
        ));
    }

    #[test]
    fn an_empty_snapshot_matches_only_the_empty_root() {
        let snapshot = ShardSnapshot::new(NETWORK, leaf_with_root(TreeHash::zero()), filters());
        assert!(snapshot.finish().is_ok());
        assert!(matches!(
            ShardState::new().snapshot().finish(),
            Err(SnapshotError::RootMismatch { .. })
        ));
    }

    #[test]
    fn a_template_whose_metadata_disagrees_with_its_hash_is_malformed() {
        let state = ShardState::new();
        let mut tampered = state.substates.clone();
        let index = tampered
            .iter()
            .position(|substate| !substate.substate.as_ref().unwrap().template_metadata.is_empty())
            .unwrap();
        let mut data = SubstateData::try_from(tampered[index].substate.take().unwrap()).unwrap();
        data.value = SubstateValueOrHash::Hash(Hash32::from_array([1; 32]));
        tampered[index].substate = Some(data.into());
        let mut snapshot = state.snapshot();
        assert!(matches!(
            state.add_pages(&mut snapshot, &tampered, 100),
            Err(SnapshotError::Malformed(_))
        ));
    }
}
