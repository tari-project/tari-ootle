//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt,
    fmt::Debug,
};

use tari_jellyfish::{JmtStorageError, Node, NodeKey, StaleTreeNode, TreeNode, TreeStoreReader, TreeStoreWriter};

#[derive(Debug, Default)]
pub struct MemoryTreeStore<P> {
    pub nodes: HashMap<NodeKey, TreeNode<P>>,
    pub stale_nodes: Vec<StaleTreeNode>,
}

impl<P> MemoryTreeStore<P> {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            stale_nodes: Vec::new(),
        }
    }

    /// Removes every recorded stale node. A [`StaleTreeNode::Subtree`] removes the node and all of its descendants,
    /// even those still shared with a newer root; see [`TreeStoreWriter::record_stale_tree_node`].
    pub fn clear_stale_nodes(&mut self) {
        // Collect every key before removing any, so that a node removed by one record can not hide the descendants
        // of an overlapping `Subtree` record.
        let mut to_remove = HashSet::new();
        let mut expanded = HashSet::new();
        for stale in self.stale_nodes.drain(..) {
            match stale {
                StaleTreeNode::Node(key) => {
                    to_remove.insert(key);
                },
                StaleTreeNode::Subtree(key) => {
                    // Iterative so that a deep (or corrupt) tree cannot overflow the stack. A key is only expanded
                    // the first time it is seen, so this terminates.
                    let mut stack = vec![key];
                    while let Some(key) = stack.pop() {
                        if !expanded.insert(key.clone()) {
                            continue;
                        }
                        if let Some(Node::Internal(internal_node)) = self.nodes.get(&key).map(TreeNode::as_node) {
                            for (nibble, child) in internal_node.children_sorted() {
                                // A child past the maximum path length cannot have been stored
                                if let Ok(child_key) = key.gen_child_node_key(child.version, nibble) {
                                    stack.push(child_key);
                                }
                            }
                        }
                        to_remove.insert(key);
                    }
                },
            }
        }
        for key in to_remove {
            self.nodes.remove(&key);
        }
    }
}

impl<P: Clone> TreeStoreReader<P> for MemoryTreeStore<P> {
    fn get_node(&self, key: &NodeKey) -> Result<Node<P>, JmtStorageError> {
        self.nodes
            .get(key)
            .map(|node| node.clone().into_node())
            .ok_or_else(|| JmtStorageError::NotFound(key.clone()))
    }
}

impl<P> TreeStoreWriter<P> for MemoryTreeStore<P> {
    fn insert_node(&mut self, key: NodeKey, node: Node<P>) -> Result<(), JmtStorageError> {
        match self.nodes.entry(key) {
            Entry::Occupied(entry) => Err(JmtStorageError::Conflict(entry.key().clone())),
            Entry::Vacant(entry) => {
                entry.insert(TreeNode::new_latest(node));
                Ok(())
            },
        }
    }

    fn record_stale_tree_node(&mut self, stale: StaleTreeNode) -> Result<(), JmtStorageError> {
        self.stale_nodes.push(stale);
        Ok(())
    }
}

impl<P: Debug> fmt::Display for MemoryTreeStore<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "MemoryTreeStore")?;
        writeln!(f, "  Nodes:")?;
        let mut store = self.nodes.iter().collect::<Vec<_>>();
        store.sort_by_key(|(key, _)| *key);
        for (key, node) in store {
            writeln!(f, "    {}: {:?}", key, node)?;
        }
        writeln!(f, "  Stale Nodes:")?;
        for stale in &self.stale_nodes {
            writeln!(f, "    {}", stale.as_node_key())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tari_jellyfish::{Child, Children, InternalNode, LeafKey, LeafNode, Nibble, NodeType, jmt_node_hash};

    use super::*;

    #[test]
    fn insert_node_refuses_to_overwrite() {
        let mut mem = MemoryTreeStore::<u64>::new();
        let key = NodeKey::new_empty_path(1);
        let first = Node::Leaf(LeafNode::new(LeafKey::new(jmt_node_hash(&1)), jmt_node_hash(&10), 1, 1));
        mem.insert_node(key.clone(), first.clone()).unwrap();

        let err = mem.insert_node(key.clone(), Node::Null).unwrap_err();
        assert!(matches!(&err, JmtStorageError::Conflict(k) if *k == key), "{err:?}");
        assert_eq!(mem.get_node(&key).unwrap(), first);
    }

    #[test]
    fn clear_stale_nodes_removes_entire_subtree() {
        let mut mem = MemoryTreeStore::<u64>::new();
        let root_key = NodeKey::new_empty_path(1);
        let nibble0 = Nibble::try_from(0u8).unwrap();
        let nibble1 = Nibble::try_from(1u8).unwrap();
        let child_key0 = root_key.gen_child_node_key(1, nibble0).unwrap();
        let child_key1 = root_key.gen_child_node_key(1, nibble1).unwrap();
        let leaf0 = Node::Leaf(LeafNode::new(LeafKey::new(jmt_node_hash(&1)), jmt_node_hash(&10), 1, 1));
        let leaf1 = Node::Leaf(LeafNode::new(LeafKey::new(jmt_node_hash(&2)), jmt_node_hash(&20), 2, 1));
        let mut children = Children::default();
        children.insert(nibble0, Child::try_new(jmt_node_hash(&1), 1, NodeType::Leaf).unwrap());
        children.insert(nibble1, Child::try_new(jmt_node_hash(&2), 1, NodeType::Leaf).unwrap());
        let root = Node::Internal(InternalNode::try_new(children).unwrap());

        mem.insert_node(root_key.clone(), root).unwrap();
        mem.insert_node(child_key0.clone(), leaf0).unwrap();
        mem.insert_node(child_key1.clone(), leaf1).unwrap();
        mem.record_stale_tree_node(StaleTreeNode::Node(root_key.clone()))
            .unwrap();
        mem.record_stale_tree_node(StaleTreeNode::Subtree(root_key.clone()))
            .unwrap();
        mem.record_stale_tree_node(StaleTreeNode::Subtree(child_key0.clone()))
            .unwrap();

        mem.clear_stale_nodes();

        assert!(matches!(mem.get_node(&root_key), Err(JmtStorageError::NotFound(_))));
        assert!(matches!(mem.get_node(&child_key0), Err(JmtStorageError::NotFound(_))));
        assert!(matches!(mem.get_node(&child_key1), Err(JmtStorageError::NotFound(_))));
    }
}
