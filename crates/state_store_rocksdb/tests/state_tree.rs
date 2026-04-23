//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod helpers;

use helpers::assert_eq_debug;
use tari_ootle_common_types::{ShardGroup, optional::Optional, shard::Shard};
use tari_ootle_storage::{StateStore, StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};
use tari_state_store_rocksdb::DatabaseOptions;
use tari_state_tree::{NibblePath, Node, NodeKey, StaleTreeNode, StateTreePayload};

use crate::helpers::{create_rocksdb_with_opts, num_preshards};

#[test]
fn state_tree_rocksdb() {
    let (db, _tmp) = create_rocksdb_with_opts(DatabaseOptions::default().with_state_history_length(0));
    let timer = std::time::Instant::now();
    state_tree_operations(db, 10_000);
    println!("state_tree_rocksdb: 10_000 nodes: {:?}", timer.elapsed());
}

fn state_tree_operations(db: impl StateStore, num_nodes: usize) {
    const SHARD: Shard = Shard::first();

    // state_tree
    let nodes = gen_nodes(1, num_nodes / 2)
        .chain(gen_nodes(2, num_nodes / 2))
        .collect::<Vec<_>>();
    db.with_write_tx(|tx| {
        for (key, value) in &nodes {
            tx.state_tree_nodes_batch_insert(SHARD, vec![(key.clone(), value.clone())])
                .unwrap();
        }
        Ok::<_, StorageError>(())
    })
    .unwrap();

    db.with_read_tx(|tx| {
        for (key, value) in &nodes {
            let res = tx.state_tree_nodes_get(SHARD, key).unwrap();
            assert_eq_debug(&res, value);
        }
        Ok::<_, StorageError>(())
    })
    .unwrap();

    db.with_write_tx(|tx| {
        tx.state_tree_nodes_record_stale_tree_nodes(
            SHARD,
            2,
            nodes[..100]
                .iter()
                .map(|(k, _)| StaleTreeNode::Node(k.clone()))
                .collect(),
        )
        .unwrap();
        for shard in ShardGroup::all_shards(num_preshards()).shard_iter() {
            tx.state_tree_shard_versions_set(shard, 100).unwrap();
        }
        Ok::<_, StorageError>(())
    })
    .unwrap();

    let n = db
        .with_write_tx(|tx| tx.state_tree_nodes_clear_stale(num_preshards()))
        .unwrap();
    assert_eq!(n, 100);
    db.with_read_tx(|tx| {
        // Stale nodes are gone
        for (key, _) in &nodes[..100] {
            let res = tx.state_tree_nodes_get(SHARD, key).optional().unwrap();
            assert!(res.is_none());
        }
        for (key, _) in &nodes[100..] {
            let res = tx.state_tree_nodes_get(SHARD, key).optional().unwrap();
            assert!(res.is_some());
        }

        for shard in ShardGroup::all_shards(num_preshards()).shard_iter() {
            let version = tx.state_tree_versions_get_latest(shard).unwrap().expect("version");
            assert_eq!(version, 100);
        }
        Ok::<_, StorageError>(())
    })
    .unwrap();
}

fn gen_nodes(version: u64, num: usize) -> impl Iterator<Item = (NodeKey, Node<StateTreePayload>)> {
    (0..num as u64).map(move |i| {
        let node = Node::Null;
        // No possibility of key collisions
        let path = NibblePath::new_even(i.to_be_bytes().to_vec());
        let node_key = NodeKey::new(version, path);
        (node_key, node)
    })
}

#[test]
fn truncate_to_version_removes_newer_versions_and_resets_pointer() {
    const SHARD: Shard = Shard::first();
    let (db, _tmp) = create_rocksdb_with_opts(DatabaseOptions::default().with_state_history_length(0));

    // Seed five versions' worth of nodes (10 per version).
    let per_version = 10usize;
    #[expect(clippy::type_complexity)]
    let all_nodes: Vec<(u64, Vec<(NodeKey, Node<StateTreePayload>)>)> =
        (1u64..=5).map(|v| (v, gen_nodes(v, per_version).collect())).collect();

    db.with_write_tx(|tx| {
        for (_, nodes) in &all_nodes {
            tx.state_tree_nodes_batch_insert(SHARD, nodes.clone()).unwrap();
        }
        // Record stale markers at versions 3, 4, 5 — these should be truncated.
        for v in [3, 4, 5] {
            tx.state_tree_nodes_record_stale_tree_nodes(SHARD, v, vec![StaleTreeNode::Node(
                all_nodes[0].1[0].0.clone(),
            )])
            .unwrap();
        }
        // And a stale marker at version 2 that should survive.
        tx.state_tree_nodes_record_stale_tree_nodes(SHARD, 2, vec![StaleTreeNode::Node(all_nodes[0].1[0].0.clone())])
            .unwrap();
        tx.state_tree_shard_versions_set(SHARD, 5).unwrap();
        Ok::<_, StorageError>(())
    })
    .unwrap();

    let stats = db
        .with_write_tx(|tx| tx.state_tree_truncate_to_version(SHARD, 2))
        .unwrap();

    // 3 versions × 10 nodes per version = 30 nodes deleted.
    assert_eq!(stats.nodes_deleted, 30);
    // Stale records at versions 3, 4, 5 deleted.
    assert_eq!(stats.stale_records_deleted, 3);

    db.with_read_tx(|tx| {
        // Versions 1 and 2 survive.
        for (v, nodes) in &all_nodes[..2] {
            for (key, _) in nodes {
                let got = tx.state_tree_nodes_get(SHARD, key).optional().unwrap();
                assert!(got.is_some(), "node at v{v} was unexpectedly deleted");
            }
        }
        // Versions 3, 4, 5 are gone.
        for (v, nodes) in &all_nodes[2..] {
            for (key, _) in nodes {
                let got = tx.state_tree_nodes_get(SHARD, key).optional().unwrap();
                assert!(got.is_none(), "node at v{v} survived truncation");
            }
        }

        // Version pointer reset to 2.
        let latest = tx.state_tree_versions_get_latest(SHARD).unwrap();
        assert_eq!(latest, Some(2));
        Ok::<_, StorageError>(())
    })
    .unwrap();
}

#[test]
fn truncate_to_version_is_shard_scoped() {
    let (db, _tmp) = create_rocksdb_with_opts(DatabaseOptions::default().with_state_history_length(0));
    let shard_a = Shard::first();
    let shard_b = Shard::from_u32(42);

    let nodes_v3 = gen_nodes(3, 5).collect::<Vec<_>>();

    db.with_write_tx(|tx| {
        tx.state_tree_nodes_batch_insert(shard_a, nodes_v3.clone()).unwrap();
        tx.state_tree_nodes_batch_insert(shard_b, nodes_v3.clone()).unwrap();
        tx.state_tree_shard_versions_set(shard_a, 3).unwrap();
        tx.state_tree_shard_versions_set(shard_b, 3).unwrap();
        Ok::<_, StorageError>(())
    })
    .unwrap();

    let stats = db
        .with_write_tx(|tx| tx.state_tree_truncate_to_version(shard_a, 1))
        .unwrap();
    assert_eq!(stats.nodes_deleted, 5);

    db.with_read_tx(|tx| {
        // shard_b untouched.
        for (key, _) in &nodes_v3 {
            assert!(tx.state_tree_nodes_get(shard_b, key).optional().unwrap().is_some());
        }
        assert_eq!(tx.state_tree_versions_get_latest(shard_b).unwrap(), Some(3));
        // shard_a truncated.
        for (key, _) in &nodes_v3 {
            assert!(tx.state_tree_nodes_get(shard_a, key).optional().unwrap().is_none());
        }
        assert_eq!(tx.state_tree_versions_get_latest(shard_a).unwrap(), Some(1));
        Ok::<_, StorageError>(())
    })
    .unwrap();
}
