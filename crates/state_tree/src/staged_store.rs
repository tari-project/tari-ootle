//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{BTreeMap, VecDeque};

use jmt::{
    storage::{LeafNode, NibblePath, Node, NodeKey, TreeReader},
    KeyHash,
    OwnedValue,
    Version,
};
use log::*;

use crate::{
    diff::{StateHashTreeDiff, StateTreeNodeBatch, StateTreeStaleNodeIndex},
    StateTreeError,
    TreeStoreWriter,
};

const LOG_TARGET: &str = "tari::ootle::consensus::sharded_state_tree";

pub struct StagedTreeStore<'s, S> {
    readable_store: &'s S,
    preceding_pending_state: BTreeMap<NodeKey, Node>,
    new_tree_nodes: BTreeMap<NodeKey, Node>,
    new_values: BTreeMap<(Version, KeyHash), Option<OwnedValue>>,
    new_stale_nodes: Vec<StateTreeStaleNodeIndex>,
}

impl<'s, S: TreeReader> StagedTreeStore<'s, S> {
    pub fn new(readable_store: &'s S) -> Self {
        Self {
            readable_store,
            preceding_pending_state: BTreeMap::new(),
            new_tree_nodes: BTreeMap::new(),
            new_values: BTreeMap::new(),
            new_stale_nodes: Vec::new(),
        }
    }

    pub fn apply_pending_diff(&mut self, diff: StateHashTreeDiff) {
        self.preceding_pending_state.extend(diff.new_nodes.nodes);
        self.new_values.extend(diff.new_nodes.values);

        for stale in &diff.stale_tree_nodes {
            trace!(target: LOG_TARGET, "PENDING DELETE: node {:?}", stale.node_key);
            if self.preceding_pending_state.remove(&stale.node_key).is_some() {
                trace!(target: LOG_TARGET, "PENDING DELETE: node {:?} removed", stale.node_key);
            }
        }
    }

    pub fn into_diff(self) -> StateHashTreeDiff {
        StateHashTreeDiff {
            new_nodes: StateTreeNodeBatch {
                nodes: self.new_tree_nodes,
                values: self.new_values,
            },
            stale_tree_nodes: self.new_stale_nodes.into_iter().collect(),
        }
    }
}

impl<S: TreeReader> TreeReader for StagedTreeStore<'_, S> {
    fn get_node_option(&self, node_key: &NodeKey) -> anyhow::Result<Option<Node>> {
        if let Some(node) = self.new_tree_nodes.get(node_key).cloned() {
            return Ok(Some(node));
        }
        if let Some(node) = self.preceding_pending_state.get(node_key).cloned() {
            return Ok(Some(node));
        }

        self.readable_store.get_node_option(node_key)
    }

    fn get_value_option(&self, max_version: Version, key_hash: KeyHash) -> anyhow::Result<Option<OwnedValue>> {
        for ((version, hash), value) in self.new_values.range(..=(max_version, key_hash)) {
            if *hash == key_hash && *version <= max_version {
                return Ok(value.clone());
            }
        }
        self.readable_store.get_value_option(max_version, key_hash)
    }

    fn get_rightmost_leaf(&self) -> anyhow::Result<Option<(NodeKey, LeafNode)>> {
        for (key, node) in self.new_tree_nodes.iter().rev() {
            if let Node::Leaf(leaf) = node {
                return Ok(Some((key.clone(), leaf.clone())));
            }
        }

        for (key, name) in self.preceding_pending_state.iter().rev() {
            if let Node::Leaf(leaf) = name {
                return Ok(Some((key.clone(), leaf.clone())));
            }
        }

        self.readable_store.get_rightmost_leaf()
    }
}

// impl<S> TreeStoreWriter for StagedTreeStore<'_, S> {
//     fn insert_node(&mut self, key: NodeKey, node: Node) -> Result<(), StateTreeError> {
//         if self.new_tree_nodes.insert(key.clone(), node).is_some() {
//             return Err(StateTreeError::Conflict(key));
//         }
//         Ok(())
//     }
//
//     fn record_stale_tree_node(&mut self, stale: StateTreeStaleNodeIndex) -> Result<(), StateTreeError> {
//         // Prune staged tree nodes immediately from preceding_pending_state.
//         // let mut remove_queue = VecDeque::new();
//         // remove_queue.push_front(stale.node_key.clone());
//         // while let Some(key) = remove_queue.pop_front() {
//         //     if let Some(node) = self.preceding_pending_state.remove(&key) {
//         //         match node {
//         //             Node::Internal(node) => {
//         //                 for (nibble, child) in node.children_unsorted() {
//         //                     let num_nibbles = key.nibble_path().num_nibbles();
//         //                     let mut node_nibble_path = key.nibble_path();//.nibbles().map(|a| a.as_usize() as
// u8).collect::<Vec<_>>();         //                     let mut nibble_bytes = key.nibble_path().nibbles().map(|a|
// a.as_usize() as u8).collect::<Vec<_>>();         //                     if num_nibbles % 2 == 0 {
//         //                         nibble_bytes.push(u8::from(nibble) << 4);
//         //                     } else {
//         //                         nibble_bytes[num_nibbles / 2] |= u8::from(nibble);
//         //                     }
//         //                     node_nibble_path.push(nibble.as_usize());
//         //                     NodeKey::new(child.version, NibblePath::new(node_nibble_path)
//         //                     remove_queue.push_back(key.gen_child_node_key(child.version, nibble));
//         //                 }
//         //             },
//         //             Node::Leaf(_) | Node::Null => {},
//         //         }
//         //     }
//         // }
//         //
//         // self.new_stale_nodes.push(stale);
//         Ok(())
//     }
// }
