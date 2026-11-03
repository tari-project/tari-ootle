//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{optional::Optional, shard::Shard};
use tari_ootle_p2p::proto::rpc as proto;
use tari_ootle_storage::{
    ShardScopedTreeStoreReader,
    StateStoreReadTransaction,
    StorageError,
    consensus_models::SubstateValueFilterFlags,
};
use tari_state_tree::{
    JellyfishMerkleTree,
    JmtHashScheme,
    JmtStorageError,
    LeafKey,
    NodeKey,
    SparseMerkleProofExt,
    Version,
};

/// The most substates one `GetShardSnapshot` page returns.
pub const MAX_SNAPSHOT_PAGE_SUBSTATES: usize = 1000;

/// The most encoded substate bytes one `GetShardSnapshot` page returns, past its first substate. The rest of the
/// response payload limit is left for the response's other fields and its envelope.
pub const MAX_SNAPSHOT_PAGE_BYTES: usize = 4 * 1024 * 1024;

const _: () = assert!(
    MAX_SNAPSHOT_PAGE_BYTES + 1024 * 1024 <= tari_rpc_framework::max_response_payload_size(),
    "a full snapshot page and its largest substate must fit one RPC response"
);

/// The most leaves a page reads from the tree at once.
const LEAF_READ_CHUNK: usize = 64;

/// The field number of `GetShardSnapshotResponse.substates`, which frames each substate in the encoded response.
const SNAPSHOT_SUBSTATES_TAG: u32 = 1;

#[derive(Debug)]
pub enum ShardSnapshotPage {
    /// This node does not hold the shard's tree at the requested version.
    Unavailable,
    Page {
        substates: Vec<proto::SnapshotSubstate>,
        is_last: bool,
        /// The proof of the last substate's leaf, if the page has any.
        last_leaf_proof: Option<SparseMerkleProofExt>,
    },
}

/// The substates `shard`'s tree holds at `state_version`, in ascending leaf key order from the first key above
/// `after`: at most `limit`, and no more than [`MAX_SNAPSHOT_PAGE_BYTES`] encode to past the first.
///
/// Every substate is returned, in the form `value_filters` selects or as its state hash, so that the caller can
/// rebuild the shard's root from the page's leaves.
pub fn read_shard_snapshot<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    shard: Shard,
    state_version: Version,
    after: Option<&LeafKey>,
    limit: usize,
    value_filters: SubstateValueFilterFlags,
) -> Result<ShardSnapshotPage, StorageError> {
    // A version's root goes stale no later than any node under it, so a held root normally means a whole tree. A
    // node pruned while the page is read is reported the same way, below.
    if tx
        .state_tree_nodes_get(shard, &NodeKey::new_empty_path(state_version))
        .optional()?
        .is_none()
    {
        return Ok(ShardSnapshotPage::Unavailable);
    }
    let tree_store = ShardScopedTreeStoreReader::new(tx, shard);
    let jmt = JellyfishMerkleTree::new(&tree_store, JmtHashScheme::V1);
    let tree_error = |e: JmtStorageError| StorageError::QueryError {
        reason: format!("read the tree of {shard} at state version {state_version}: {e}"),
    };
    let value_filters = value_filters | SubstateValueFilterFlags::ALL_HASHES;
    let mut substates = Vec::new();
    let mut page_bytes = 0usize;
    let mut last_key = after.copied();
    let mut is_full = false;
    let mut is_exhausted = false;
    // Leaves are read a chunk at a time, so a page the byte budget fills early reads few beyond it
    'page: while substates.len() < limit {
        let chunk = LEAF_READ_CHUNK.min(limit - substates.len());
        let leaves = match jmt.get_leaves_after(state_version, last_key.as_ref(), chunk) {
            Ok(leaves) => leaves,
            Err(JmtStorageError::NotFound(_)) => return Ok(ShardSnapshotPage::Unavailable),
            Err(e) => return Err(tree_error(e)),
        };
        is_exhausted = leaves.len() < chunk;
        for leaf in leaves {
            let record = tx.substates_get(leaf.payload())?;
            let created_epoch = record.created.at_epoch;
            let substate_id = record.substate_id.clone();
            let data = value_filters.serve(record).ok_or_else(|| StorageError::QueryError {
                reason: format!("{substate_id} is served in no form under {value_filters:?}"),
            })?;
            let substate = proto::SnapshotSubstate {
                substate: Some(data.into()),
                created_epoch: Some(created_epoch.into()),
            };
            let substate_bytes = prost::encoding::message::encoded_len(SNAPSHOT_SUBSTATES_TAG, &substate);
            if !substates.is_empty() && page_bytes.saturating_add(substate_bytes) > MAX_SNAPSHOT_PAGE_BYTES {
                is_full = true;
                break 'page;
            }
            page_bytes = page_bytes.saturating_add(substate_bytes);
            substates.push(substate);
            last_key = Some(*leaf.leaf_key());
        }
        if is_exhausted {
            break;
        }
    }
    let last_key = last_key.filter(|_| !substates.is_empty());
    let last_leaf_proof = match last_key {
        Some(key) => match jmt.get_with_proof_ext(key.as_ref(), state_version) {
            Ok((_, proof)) => Some(proof),
            Err(JmtStorageError::NotFound(_)) => return Ok(ShardSnapshotPage::Unavailable),
            Err(e) => return Err(tree_error(e)),
        },
        None => None,
    };
    Ok(ShardSnapshotPage::Page {
        substates,
        is_last: is_exhausted && !is_full,
        last_leaf_proof,
    })
}

/// The encoded size of `substates` as the substates of a `GetShardSnapshotResponse`.
#[cfg(test)]
fn encoded_substates_len(substates: &[proto::SnapshotSubstate]) -> usize {
    substates
        .iter()
        .map(|substate| prost::encoding::message::encoded_len(SNAPSHOT_SUBSTATES_TAG, substate))
        .sum()
}

#[cfg(test)]
mod tests {
    use tari_engine_types::{
        published_template::{PublishedTemplate, PublishedTemplateAddress},
        substate::{SubstateId, SubstateValue},
    };
    use tari_ootle_common_types::{Epoch, SubstateVersion};
    use tari_ootle_storage::{
        ShardScopedTreeStoreWriter,
        StateStore,
        StateStoreWriteTransaction,
        consensus_models::{SubstateData, SubstateTransition, SubstateUpdateBatch, SubstateValueOrHash},
    };
    use tari_ootle_transaction::Network;
    use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
    use tari_state_tree::{
        SortedLeafRootBuilder,
        SpreadPrefixStateTree,
        TreeHash,
        key_mapper::{DbKeyMapper, SpreadPrefixKeyMapper},
    };
    use tari_template_lib_types::{Hash32, crypto::RistrettoPublicKeyBytes};

    use super::*;

    const NETWORK: Network = Network::LocalNet;

    fn template(seed: u16, binary_len: usize) -> (SubstateId, SubstateValue) {
        let mut address = [0u8; 32];
        address[..2].copy_from_slice(&seed.to_le_bytes());
        let id = SubstateId::Template(PublishedTemplateAddress::from(Hash32::from_array(address)));
        let value = SubstateValue::Template(PublishedTemplate {
            template_name: "snapshot".try_into().unwrap(),
            author: RistrettoPublicKeyBytes::default(),
            binary: vec![1; binary_len].try_into().unwrap(),
            at_epoch: 0,
            metadata_hash: None,
        });
        (id, value)
    }

    /// Commits `num_substates` templates to the global shard at version 1 and destroys the first `num_destroyed` at
    /// version 2, writing each version's substates and tree. Returns the root at each version.
    fn store_with_templates(
        num_substates: u16,
        num_destroyed: u16,
        binary_len: usize,
    ) -> (tempfile::TempDir, RocksDbStateStore<String>, [TreeHash; 2]) {
        let dir = tempfile::tempdir().unwrap();
        let store = RocksDbStateStore::<String>::open(dir.path().join("db"), DatabaseOptions::default()).unwrap();
        let shard = Shard::global();
        let roots = store
            .with_write_tx(|tx| {
                let mut ups = SubstateUpdateBatch::new(NETWORK, Epoch(1));
                let transitions = ups.with_transition(shard, 1);
                for seed in 0..num_substates {
                    let (id, value) = template(seed, binary_len);
                    transitions.push(SubstateTransition::Up {
                        id,
                        version: SubstateVersion::ZERO,
                        substate_or_hash: SubstateValueOrHash::Value(Box::new(value)),
                    });
                }
                let changes = ups.updates[&shard][&1]
                    .iter()
                    .map(|transition| transition.to_tree_change(NETWORK, Epoch(1)))
                    .collect::<Vec<_>>();
                tx.substates_commit_batch(ups)?;
                let mut tree_store = ShardScopedTreeStoreWriter::new(tx, shard);
                let root_v1 = SpreadPrefixStateTree::new(&mut tree_store)
                    .batch_put_substate_changes(None, 1, changes)
                    .unwrap();

                let mut downs = SubstateUpdateBatch::new(NETWORK, Epoch(2));
                let transitions = downs.with_transition(shard, 2);
                for seed in 0..num_destroyed {
                    transitions.push(SubstateTransition::Down {
                        id: tari_ootle_common_types::VersionedSubstateId::new(
                            template(seed, binary_len).0,
                            SubstateVersion::ZERO,
                        ),
                    });
                }
                let changes = downs.updates[&shard][&2]
                    .iter()
                    .map(|transition| transition.to_tree_change(NETWORK, Epoch(2)))
                    .collect::<Vec<_>>();
                tx.substates_commit_batch(downs)?;
                let mut tree_store = ShardScopedTreeStoreWriter::new(tx, shard);
                let root_v2 = SpreadPrefixStateTree::new(&mut tree_store)
                    .batch_put_substate_changes(Some(1), 2, changes)
                    .unwrap();
                tx.state_tree_shard_versions_set(shard, 2)?;
                Ok::<_, StorageError>([root_v1, root_v2])
            })
            .unwrap();
        (dir, store, roots)
    }

    /// Pages through the global shard's snapshot at `state_version`, as a caller does, checking each page against
    /// `root` through its last leaf, and rebuilds the root.
    fn page_through(
        store: &RocksDbStateStore<String>,
        state_version: Version,
        root: TreeHash,
        limit: usize,
        value_filters: SubstateValueFilterFlags,
    ) -> (Vec<SubstateData>, TreeHash, usize) {
        let tx = store.create_read_tx().unwrap();
        let mut builder = SortedLeafRootBuilder::new(JmtHashScheme::V1);
        let mut all = Vec::new();
        let mut num_pages = 0;
        loop {
            let page = read_shard_snapshot(
                &tx,
                Shard::global(),
                state_version,
                builder.last_key(),
                limit,
                value_filters,
            )
            .unwrap();
            let ShardSnapshotPage::Page {
                substates,
                is_last,
                last_leaf_proof,
            } = page
            else {
                panic!("the tree at v{state_version} is held");
            };
            assert!(encoded_substates_len(&substates) <= MAX_SNAPSHOT_PAGE_BYTES || substates.len() == 1);
            num_pages += 1;
            for substate in substates {
                let created_epoch = Epoch::from(substate.created_epoch.unwrap());
                let data = SubstateData::try_from(substate.substate.unwrap()).unwrap();
                let id = data.as_versioned_substate_id_ref().to_owned();
                builder
                    .push(
                        SpreadPrefixKeyMapper::map_to_leaf_key(&id),
                        &TreeHash::new(data.to_value_hash(NETWORK, created_epoch).into_array()),
                    )
                    .unwrap();
                all.push(data);
            }
            if let Some(proof) = last_leaf_proof {
                let proof = proof.into_compact(JmtHashScheme::V1);
                assert_eq!(builder.root_through_last(proof.siblings()).unwrap(), root);
            }
            if is_last {
                return (all, builder.finish(), num_pages);
            }
        }
    }

    #[test]
    fn its_pages_rebuild_the_root_of_the_version() {
        // More substates than a leaf read chunk, so pages span chunks
        let (_dir, store, [root_v1, root_v2]) = store_with_templates(150, 15, 16);
        let filters = SubstateValueFilterFlags::TRANSACTION_RECEIPT;
        for limit in [1, 7, LEAF_READ_CHUNK, LEAF_READ_CHUNK + 1, 150, 1000] {
            let (substates, root, num_pages) = page_through(&store, 1, root_v1, limit, filters);
            assert_eq!(substates.len(), 150, "limit {limit}");
            assert_eq!(root, root_v1, "limit {limit}");
            assert_eq!(num_pages, 150 / limit + 1, "limit {limit}");
        }
        let (substates, root, _) = page_through(&store, 2, root_v2, 6, filters);
        assert_eq!(substates.len(), 135);
        assert_eq!(root, root_v2);
        // Unselected substates are served as their hash alone
        assert!(substates.iter().all(|data| data.value.hash().is_some()));
    }

    #[test]
    fn it_serves_selected_substates_in_the_form_the_filters_select() {
        let (_dir, store, [root_v1, _]) = store_with_templates(5, 0, 16);
        let (substates, root, _) = page_through(&store, 1, root_v1, 100, SubstateValueFilterFlags::TEMPLATE_METADATA);
        assert_eq!(root, root_v1);
        assert!(substates.iter().all(|data| data.template_metadata.is_some()));
        assert!(substates.iter().all(|data| data.value.hash().is_some()));

        let (substates, root, _) = page_through(&store, 1, root_v1, 100, SubstateValueFilterFlags::TEMPLATE);
        assert_eq!(root, root_v1);
        assert!(substates.iter().all(|data| data.value.value().is_some()));
    }

    #[test]
    fn a_page_is_bounded_by_its_byte_budget() {
        const BINARY_LEN: usize = 1024 * 1024;
        let (_dir, store, [root_v1, _]) = store_with_templates(9, 0, BINARY_LEN);
        let (substates, root, num_pages) = page_through(&store, 1, root_v1, 1000, SubstateValueFilterFlags::TEMPLATE);
        assert_eq!(substates.len(), 9);
        assert_eq!(root, root_v1);
        assert!(num_pages > 2, "9 MiB of templates take {num_pages} pages");
    }

    #[test]
    fn a_version_whose_tree_is_not_held_is_unavailable() {
        let (_dir, store, _) = store_with_templates(5, 0, 16);
        let tx = store.create_read_tx().unwrap();
        for (shard, version) in [(Shard::global(), 3), (Shard::from_u32(1), 1)] {
            let page = read_shard_snapshot(
                &tx,
                shard,
                version,
                None,
                10,
                SubstateValueFilterFlags::TRANSACTION_RECEIPT,
            )
            .unwrap();
            assert!(matches!(page, ShardSnapshotPage::Unavailable), "{shard} v{version}");
        }
    }
}
