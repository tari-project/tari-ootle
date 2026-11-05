//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod helpers;

use helpers::{create_rocksdb_with_opts, num_preshards, substate_id_seed};
use indexmap::IndexMap;
use tari_ootle_common_types::{
    Epoch,
    ShardGroup,
    SubstateVersion,
    VersionedSubstateId,
    optional::Optional,
    shard::Shard,
};
use tari_ootle_storage::{
    ShardScopedTreeStoreReader,
    ShardScopedTreeStoreWriter,
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StateTreeRetention,
    StorageError,
    consensus_models::{BlockHeaderMetadata, EndOfEpochCommand, EpochCheckpoint, TreeRootSummary},
};
use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};
use tari_state_tree::{
    JellyfishMerkleTree,
    JmtHashScheme,
    KeyedProofTree,
    LeafKey,
    Node,
    NodeKey,
    SpreadPrefixStateTree,
    StaleTreeNode,
    SubstateTreeChange,
    TransitionAccumulator,
    TreeHash,
    Version,
};
use tari_template_lib::types::Hash32;

const SHARD: Shard = Shard::first();
const HISTORY: u64 = 2;
const LATEST: Version = 24;
const NUM_SUBSTATES: u32 = 8;

fn create_store() -> (RocksDbStateStore<String>, tempfile::TempDir) {
    create_rocksdb_with_opts(DatabaseOptions::default().with_state_history_length(HISTORY))
}

fn value_hash(seed: u32, version: u32) -> Hash32 {
    let mut hash = [0u8; 32];
    hash[..4].copy_from_slice(&seed.to_be_bytes());
    hash[4..8].copy_from_slice(&version.to_be_bytes());
    Hash32::from_array(hash)
}

fn substate(seed: u32, version: u32) -> VersionedSubstateId {
    VersionedSubstateId::new(substate_id_seed(seed), SubstateVersion::new(u64::from(version)))
}

/// Writes versions 1 to [`LATEST`] of [`SHARD`]'s tree: version 1 creates [`NUM_SUBSTATES`] substates, and each later
/// version replaces one of them with its next version, in turn.
fn write_tree(store: &impl StateStore) {
    store
        .with_write_tx(|tx| {
            let mut substate_versions = vec![0u32; NUM_SUBSTATES as usize];
            for version in 1..=LATEST {
                let changes = if version == 1 {
                    (0..NUM_SUBSTATES)
                        .map(|seed| SubstateTreeChange::Up {
                            id: substate(seed, 0),
                            value_hash: value_hash(seed, 0),
                        })
                        .collect::<Vec<_>>()
                } else {
                    let seed = u32::try_from(version).unwrap() % NUM_SUBSTATES;
                    let current = substate_versions[seed as usize];
                    substate_versions[seed as usize] += 1;
                    vec![
                        SubstateTreeChange::Down {
                            id: substate(seed, current),
                        },
                        SubstateTreeChange::Up {
                            id: substate(seed, current + 1),
                            value_hash: value_hash(seed, current + 1),
                        },
                    ]
                };
                let mut tree_store = ShardScopedTreeStoreWriter::new(tx, SHARD);
                SpreadPrefixStateTree::new(&mut tree_store)
                    .batch_put_substate_changes(version.checked_sub(1).filter(|v| *v > 0), version, changes)
                    .unwrap();
                tree_store.set_state_version(version).unwrap();
            }
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

/// A tree's root and the key and value hash of each of its leaves.
type TreeContents = (TreeHash, Vec<(LeafKey, [u8; 32])>);

/// The root and every leaf of [`SHARD`]'s tree at `version`, or `None` if the store no longer holds its root.
fn read_tree(store: &impl StateStore, version: Version) -> Option<TreeContents> {
    store
        .with_read_tx(|tx| {
            if tx
                .state_tree_nodes_get(SHARD, &NodeKey::new_empty_path(version))
                .optional()?
                .is_none()
            {
                return Ok(None);
            }
            let tree_store = ShardScopedTreeStoreReader::new(tx, SHARD);
            let jmt = JellyfishMerkleTree::new(&tree_store, JmtHashScheme::V1);
            let root = jmt.get_root_hash(version).unwrap();
            let leaves = jmt
                .get_leaves_after(version, None, usize::MAX)
                .unwrap()
                .into_iter()
                .map(|leaf| (*leaf.leaf_key(), *leaf.value_hash()))
                .collect();
            Ok::<_, StorageError>(Some((root, leaves)))
        })
        .unwrap()
}

fn clear_stale(store: &impl StateStore, pins: &[Version]) -> usize {
    store
        .with_write_tx(|tx| {
            let mut retention = StateTreeRetention::default();
            retention.add_shard(SHARD, LATEST);
            for pin in pins {
                retention.pin(SHARD, *pin);
            }
            tx.state_tree_nodes_clear_stale(&retention, usize::MAX)
        })
        .unwrap()
}

#[test]
fn gc_keeps_the_tree_at_a_pinned_version_and_frees_it_once_unpinned() {
    const PIN: Version = 10;
    let (store, _tmp) = create_store();
    write_tree(&store);
    let pinned = read_tree(&store, PIN).unwrap();
    assert_eq!(pinned.1.len(), NUM_SUBSTATES as usize);
    let recent = (LATEST - HISTORY..=LATEST)
        .map(|v| read_tree(&store, v).unwrap())
        .collect::<Vec<_>>();

    assert!(clear_stale(&store, &[PIN]) > 0);
    assert_eq!(read_tree(&store, PIN).as_ref(), Some(&pinned));
    for version in (1..LATEST - HISTORY).filter(|v| *v != PIN) {
        assert!(
            read_tree(&store, version).is_none(),
            "the tree at v{version} is neither recent nor pinned, yet its root was kept"
        );
    }
    for (version, tree) in (LATEST - HISTORY..=LATEST).zip(&recent) {
        assert_eq!(read_tree(&store, version).as_ref(), Some(tree));
    }

    // Unpinned, the nodes kept for the pin are freed, and the recent trees are untouched
    assert!(clear_stale(&store, &[]) > 0);
    assert!(read_tree(&store, PIN).is_none());
    for (version, tree) in (LATEST - HISTORY..=LATEST).zip(&recent) {
        assert_eq!(read_tree(&store, version).as_ref(), Some(tree));
    }
    assert_eq!(clear_stale(&store, &[]), 0);
}

/// The keys of every node of [`SHARD`]'s tree at `version`.
fn node_keys_at(store: &impl StateStore, version: Version) -> Vec<NodeKey> {
    fn walk<TTx: StateStoreReadTransaction>(tx: &TTx, key: NodeKey, keys: &mut Vec<NodeKey>) {
        if let Node::Internal(node) = tx.state_tree_nodes_get(SHARD, &key).unwrap() {
            for (nibble, child) in node.children().iter() {
                walk(tx, key.gen_child_node_key(child.version, nibble).unwrap(), keys);
            }
        }
        keys.push(key);
    }
    store
        .with_read_tx(|tx| {
            let mut keys = Vec::new();
            walk(tx, NodeKey::new_empty_path(version), &mut keys);
            Ok::<_, StorageError>(keys)
        })
        .unwrap()
}

/// A pinned tree is released across as many bounded calls as it takes, each resuming where the last stopped.
#[test]
fn a_released_pin_is_freed_across_bounded_calls() {
    const PIN: Version = 10;
    const MAX_WRITES: usize = 3;
    let (store, _tmp) = create_store();
    write_tree(&store);
    let recent = read_tree(&store, LATEST).unwrap();
    clear_stale(&store, &[PIN]);

    let mut calls = 0;
    loop {
        let num_written = store
            .with_write_tx(|tx| {
                let mut retention = StateTreeRetention::default();
                retention.add_shard(SHARD, LATEST);
                tx.state_tree_nodes_clear_stale(&retention, MAX_WRITES)
            })
            .unwrap();
        calls += 1;
        if num_written < MAX_WRITES {
            break;
        }
    }
    assert!(calls > 1, "the release fit in one call, so the bound was not exercised");
    assert!(read_tree(&store, PIN).is_none());
    assert_eq!(read_tree(&store, LATEST), Some(recent));
}

#[test]
fn pins_are_released_oldest_first() {
    const OLDER: Version = 6;
    const NEWER: Version = 12;
    let (store, _tmp) = create_store();
    write_tree(&store);
    let older = read_tree(&store, OLDER).unwrap();
    let newer = read_tree(&store, NEWER).unwrap();

    clear_stale(&store, &[OLDER, NEWER]);
    assert_eq!(read_tree(&store, OLDER).as_ref(), Some(&older));
    assert_eq!(read_tree(&store, NEWER).as_ref(), Some(&newer));

    clear_stale(&store, &[NEWER]);
    assert!(read_tree(&store, OLDER).is_none());
    assert_eq!(read_tree(&store, NEWER).as_ref(), Some(&newer));

    clear_stale(&store, &[]);
    assert!(read_tree(&store, NEWER).is_none());
}

#[test]
fn a_stale_subtree_a_pinned_root_reaches_is_kept() {
    let (store, _tmp) = create_store();
    let latest = 3 + HISTORY;
    store
        .with_write_tx(|tx| {
            let mut tree_store = ShardScopedTreeStoreWriter::new(tx, SHARD);
            SpreadPrefixStateTree::new(&mut tree_store)
                .batch_put_substate_changes(
                    None,
                    1,
                    (0..NUM_SUBSTATES).map(|seed| SubstateTreeChange::Up {
                        id: substate(seed, 0),
                        value_hash: value_hash(seed, 0),
                    }),
                )
                .unwrap();
            // Version 3 drops the whole tree of version 1
            tree_store
                .record_stale_tree_nodes(3, &[StaleTreeNode::Subtree(NodeKey::new_empty_path(1))])
                .unwrap();
            tree_store.set_state_version(latest).unwrap();
            Ok::<_, StorageError>(())
        })
        .unwrap();
    let tree = read_tree(&store, 1).unwrap();
    let keys = node_keys_at(&store, 1);
    assert!(keys.len() > NUM_SUBSTATES as usize);
    let clear = |pins: &[Version]| {
        store
            .with_write_tx(|tx| {
                let mut retention = StateTreeRetention::default();
                retention.add_shard(SHARD, latest);
                for pin in pins {
                    retention.pin(SHARD, *pin);
                }
                tx.state_tree_nodes_clear_stale(&retention, usize::MAX)
            })
            .unwrap()
    };

    clear(&[2]);
    assert_eq!(read_tree(&store, 1).as_ref(), Some(&tree));
    clear(&[]);
    store
        .with_read_tx(|tx| {
            for key in &keys {
                assert!(
                    tx.state_tree_nodes_get(SHARD, key).optional()?.is_none(),
                    "{key} was kept"
                );
            }
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn truncating_below_a_deferred_stale_version_keeps_its_nodes_live() {
    let (store, _tmp) = create_store();
    let kept_key = NodeKey::new_empty_path(1);
    let latest = 3 + HISTORY;
    store
        .with_write_tx(|tx| {
            tx.state_tree_nodes_batch_insert(SHARD, &[
                (kept_key.clone(), Node::Null),
                (NodeKey::new_empty_path(2), Node::Null),
            ])?;
            tx.state_tree_nodes_record_stale_tree_nodes(SHARD, 3, &[StaleTreeNode::Node(kept_key.clone())])?;
            tx.state_tree_shard_versions_set(SHARD, latest)?;
            let mut retention = StateTreeRetention::default();
            retention.add_shard(SHARD, latest).pin(SHARD, 1);
            tx.state_tree_nodes_clear_stale(&retention, usize::MAX)?;
            // Version 3 is undone, so the node it made stale is live again at version 2
            tx.state_tree_truncate_to_version(SHARD, 2)?;
            let mut retention = StateTreeRetention::default();
            retention.add_shard(SHARD, latest);
            tx.state_tree_nodes_clear_stale(&retention, usize::MAX)?;
            Ok::<_, StorageError>(())
        })
        .unwrap();
    store
        .with_read_tx(|tx| {
            assert!(tx.state_tree_nodes_get(SHARD, &kept_key).optional()?.is_some());
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

fn checkpoint(epoch: u64, shard_versions: &[(Shard, Version)]) -> EpochCheckpoint {
    let shard_group = ShardGroup::all_shards(num_preshards());
    let key = LeafKey::new(TreeHash::new([7; 32]));
    let proof = KeyedProofTree::build([(key, TreeHash::new([7; 32]))])
        .unwrap()
        .get_proof(&key)
        .unwrap()
        .1;
    let header = tari_sidechain::SidechainBlockHeader {
        network: helpers::NETWORK.as_byte(),
        protocol_version: tari_engine_types::ProtocolVersion::at(helpers::NETWORK, Epoch(epoch)).as_u32(),
        parent_id: Default::default(),
        justify_id: Default::default(),
        height: 4,
        epoch,
        epoch_hash: Default::default(),
        shard_group: tari_sidechain::ShardGroup {
            start: shard_group.start().as_u32(),
            end_inclusive: shard_group.end().as_u32(),
        },
        proposed_by: Default::default(),
        state_merkle_root: Default::default(),
        command_merkle_root: Default::default(),
        transaction_merkle_root: Default::default(),
        signature: Default::default(),
        accumulated_data: Default::default(),
        metadata_hash: Default::default(),
    };
    let summary = shard_versions
        .iter()
        .map(|(shard, version)| {
            (*shard, TreeRootSummary {
                root_hash: TreeHash::zero(),
                state_version: *version,
                transition_acc: TransitionAccumulator::zero(),
            })
        })
        .collect::<IndexMap<_, _>>();
    EpochCheckpoint::new(
        tari_sidechain::CommandCommitProof::new(
            EndOfEpochCommand::new(Default::default()),
            tari_sidechain::SidechainBlockCommitProof {
                header,
                proof_elements: vec![],
            },
            proof,
        ),
        summary,
        BlockHeaderMetadata {
            total_leader_fee: 0,
            timestamp: 0,
            extra_data: Default::default(),
            timeout_certificate_id: None,
            is_dummy: false,
        },
    )
}

#[test]
fn retention_pins_each_shard_at_the_last_checkpoints() {
    let other = Shard::from_u32(5);
    let (store, _tmp) = create_store();
    store
        .with_write_tx(|tx| {
            tx.state_tree_shard_versions_set(SHARD, 40)?;
            tx.state_tree_shard_versions_set(other, 9)?;
            for (epoch, version) in [(1, 10), (2, 20), (3, 30)] {
                tx.epoch_checkpoint_save(&checkpoint(epoch, &[(SHARD, version), (other, 9)]))?;
            }
            Ok::<_, StorageError>(())
        })
        .unwrap();

    let read = |pinned_checkpoint_epochs| {
        store
            .with_read_tx(|tx| StateTreeRetention::read(tx, num_preshards(), pinned_checkpoint_epochs))
            .unwrap()
    };
    let pins = |retention: &StateTreeRetention, shard| {
        retention
            .get(shard)
            .unwrap()
            .pinned_versions
            .iter()
            .copied()
            .collect::<Vec<_>>()
    };
    let retention = read(2);
    assert_eq!(retention.get(SHARD).unwrap().latest_version, 40);
    assert_eq!(pins(&retention, SHARD), [20, 30]);
    assert_eq!(pins(&retention, other), [9]);
    assert!(retention.get(Shard::from_u32(6)).is_none());
    assert_eq!(pins(&read(0), SHARD), Vec::<Version>::new());
    assert_eq!(pins(&read(10), SHARD), [10, 20, 30]);
}
