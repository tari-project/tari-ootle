//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{HashMap, hash_map::Entry};

use tari_jellyfish::{JmtStorageError, Node, NodeKey, StaleTreeNode, TreeStoreReader, TreeStoreWriter};

use crate::{IndexedTreeDiff, StateHashTreeDiff};

/// A tree store that reads through the diffs of pending (uncommitted) versions to a readable store, and stages the
/// nodes the next version writes.
pub struct StagedTreeStore<'s, S, P> {
    readable_store: &'s S,
    /// Pending diffs, oldest first. A node key names one node for good, and the tree only reads nodes its current
    /// root reaches, so a node a later pending diff marks stale is never looked up and the diffs are read unpruned.
    pending_diffs: Vec<IndexedTreeDiff<P>>,
    new_tree_nodes: Vec<(NodeKey, Node<P>)>,
    new_node_positions: HashMap<NodeKey, usize>,
    new_stale_nodes: Vec<StaleTreeNode>,
}

impl<'s, S: TreeStoreReader<P>, P> StagedTreeStore<'s, S, P> {
    pub fn new(readable_store: &'s S) -> Self {
        Self {
            readable_store,
            pending_diffs: Vec::new(),
            new_tree_nodes: Vec::new(),
            new_node_positions: HashMap::new(),
            new_stale_nodes: Vec::new(),
        }
    }

    /// Layers the diff of the next pending version over those already applied.
    pub fn apply_pending_diff(&mut self, diff: IndexedTreeDiff<P>) {
        self.pending_diffs.push(diff);
    }

    pub fn into_diff(self) -> StateHashTreeDiff<P> {
        StateHashTreeDiff {
            new_nodes: self.new_tree_nodes,
            stale_tree_nodes: self.new_stale_nodes,
        }
    }
}

impl<S: TreeStoreReader<P>, P: Clone> TreeStoreReader<P> for StagedTreeStore<'_, S, P> {
    fn get_node(&self, key: &NodeKey) -> Result<Node<P>, JmtStorageError> {
        if let Some(&pos) = self.new_node_positions.get(key) {
            return Ok(self.new_tree_nodes[pos].1.clone());
        }
        if let Some(node) = self.pending_diffs.iter().rev().find_map(|diff| diff.get_node(key)) {
            return Ok(node.clone());
        }

        self.readable_store.get_node(key)
    }
}

impl<S, P> TreeStoreWriter<P> for StagedTreeStore<'_, S, P> {
    fn insert_node(&mut self, key: NodeKey, node: Node<P>) -> Result<(), JmtStorageError> {
        match self.new_node_positions.entry(key.clone()) {
            Entry::Occupied(_) => Err(JmtStorageError::Conflict(key)),
            Entry::Vacant(entry) => {
                entry.insert(self.new_tree_nodes.len());
                self.new_tree_nodes.push((key, node));
                Ok(())
            },
        }
    }

    fn record_stale_tree_node(&mut self, stale: StaleTreeNode) -> Result<(), JmtStorageError> {
        self.new_stale_nodes.push(stale);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tari_jellyfish::{LeafKey, LeafNode, NibblePath, jmt_node_hash};

    use super::*;
    use crate::memory_store::MemoryTreeStore;

    #[test]
    fn insert_node_refuses_to_overwrite_staged_node() {
        let base = MemoryTreeStore::<u64>::new();
        let mut staged = StagedTreeStore::new(&base);
        let key = NodeKey::new_empty_path(1);
        let first = Node::Leaf(LeafNode::new(LeafKey::new(jmt_node_hash(&1)), jmt_node_hash(&10), 1, 1));
        staged.insert_node(key.clone(), first.clone()).unwrap();

        let err = staged.insert_node(key.clone(), Node::Null).unwrap_err();
        assert!(matches!(&err, JmtStorageError::Conflict(k) if *k == key), "{err:?}");
        assert_eq!(staged.get_node(&key).unwrap(), first);
    }

    #[test]
    fn into_diff_preserves_insertion_order_of_new_nodes() {
        let base = MemoryTreeStore::<u64>::new();
        let mut staged = StagedTreeStore::new(&base);
        let expected: Vec<(NodeKey, Node<u64>)> = (0..16u8)
            .map(|i| {
                let key = NodeKey::new(u64::from(i), NibblePath::new_even(&[i]).unwrap());
                let node = Node::Leaf(LeafNode::new(
                    LeafKey::new(jmt_node_hash(&i)),
                    jmt_node_hash(&(i + 100)),
                    u64::from(i),
                    u64::from(i),
                ));
                staged.insert_node(key.clone(), node.clone()).unwrap();
                (key, node)
            })
            .collect();

        let diff = staged.into_diff();
        assert_eq!(diff.new_nodes, expected);
    }
}
