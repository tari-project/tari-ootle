//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::{optional::Optional, shard::Shard, ShardGroup};
use tari_dan_storage::{StateStore, StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};
use tari_state_tree::{NibblePath, Node, NodeKey, StaleTreeNode, Version};

use crate::{
    helper::{assert_eq_debug, create_rocksdb, create_sqlite},
    TEST_NUM_PRESHARDS,
};

#[test]
fn state_tree_sqlite() {
    let db = create_sqlite();
    db.foreign_keys_off().unwrap();
    state_tree_operations(db, 1000);
}

#[test]
fn state_tree_rocksdb() {
    let (db, _tmp) = create_rocksdb();
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
            tx.state_tree_nodes_insert(SHARD, key.clone(), value.clone()).unwrap();
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
        for (key, _) in &nodes[..100] {
            let stale_node = StaleTreeNode::Node(key.clone());
            tx.state_tree_nodes_record_stale_tree_node(SHARD, stale_node).unwrap();
        }
        for shard in ShardGroup::all_shards(TEST_NUM_PRESHARDS).shard_iter() {
            tx.state_tree_shard_versions_set(shard, 100).unwrap();
        }
        Ok::<_, StorageError>(())
    })
    .unwrap();

    db.with_write_tx(|tx| tx.state_tree_nodes_clear_stale(100)).unwrap();
    db.with_read_tx(|tx| {
        for (key, _) in &nodes[..100] {
            let res = tx.state_tree_nodes_get(SHARD, key).optional().unwrap();
            assert!(res.is_none());
        }
        for (key, _) in &nodes[100..] {
            let res = tx.state_tree_nodes_get(SHARD, key).optional().unwrap();
            assert!(res.is_some());
        }

        for shard in ShardGroup::all_shards(TEST_NUM_PRESHARDS).shard_iter() {
            let version = tx.state_tree_versions_get_latest(shard).unwrap().expect("version");
            assert_eq!(version, 100);
        }
        Ok::<_, StorageError>(())
    })
    .unwrap();
}

fn gen_nodes(version: u64, num: usize) -> impl Iterator<Item = (NodeKey, Node<Version>)> {
    (0..num as u64).map(move |i| {
        let node = Node::Null;
        // No possibility of key collisions
        let path = NibblePath::new_even(i.to_be_bytes().to_vec());
        let node_key = NodeKey::new(version, path);
        (node_key, node)
    })
}
