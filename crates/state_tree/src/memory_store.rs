//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::BTreeMap, fmt, fmt::Debug};

use jmt::{
    storage::{LeafNode, Node, NodeBatch, NodeKey, StaleNodeIndexBatch, TreeReader},
    KeyHash,
    OwnedValue,
    Version,
};

use crate::{helpers::write_node_key, StateTreeError, TreeStoreBatchWriter};

#[derive(Debug, Default)]
pub struct MemoryTreeStore {
    pub nodes: BTreeMap<NodeKey, Node>,
    pub values: BTreeMap<(Version, KeyHash), Option<OwnedValue>>,
    pub stale_nodes: BTreeMap<Version, StaleNodeIndexBatch>,
}

impl MemoryTreeStore {
    pub fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            values: BTreeMap::new(),
            stale_nodes: BTreeMap::new(),
        }
    }
}

impl TreeReader for MemoryTreeStore {
    fn get_node_option(&self, node_key: &NodeKey) -> anyhow::Result<Option<Node>> {
        Ok(self.nodes.get(node_key).cloned())
    }

    fn get_value_option(&self, max_version: Version, key_hash: KeyHash) -> anyhow::Result<Option<OwnedValue>> {
        for ((version, hash), value) in self.values.range(..=(max_version, key_hash)) {
            if *version <= max_version && *hash == key_hash {
                return Ok(value.clone());
            }
        }
        Ok(None)
    }

    fn get_rightmost_leaf(&self) -> anyhow::Result<Option<(NodeKey, LeafNode)>> {
        for (hash, value) in self.nodes.iter().rev() {
            if let Node::Leaf(leaf) = value {
                return Ok(Some((hash.clone(), leaf.clone())));
            }
        }
        Ok(None)
    }
}
impl TreeStoreBatchWriter for MemoryTreeStore {
    fn batch_insert_nodes(&mut self, batch: NodeBatch) -> Result<(), StateTreeError> {
        // TODO: if jmt ever adds into_parts() or makes the fields public, we can avoid cloning here
        self.nodes
            .extend(batch.nodes().iter().map(|(k, v)| (k.clone(), v.clone())));
        self.values.extend(batch.values().iter().map(|(k, v)| (*k, v.clone())));

        Ok(())
    }

    fn record_stale_tree_nodes(
        &mut self,
        version: Version,
        stale_nodes: StaleNodeIndexBatch,
    ) -> Result<(), StateTreeError> {
        self.stale_nodes.insert(version, stale_nodes);
        Ok(())
    }
}

// impl TreeStoreWriter for MemoryTreeStore {
//     fn insert_node(&mut self, key: NodeKey, node: Node) -> Result<(), StateTreeError> {
//         self.nodes.insert(key, node);
//         Ok(())
//     }
//
//     fn record_stale_tree_node(&mut self, stale: StateTreeStaleNodeIndex) -> Result<(), StateTreeError> {
//         self.stale_nodes.push(stale);
//         Ok(())
//     }
// }

impl fmt::Display for MemoryTreeStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "MemoryTreeStore")?;
        writeln!(f, "  Nodes:")?;
        for (key, node) in &self.nodes {
            write!(f, "    ")?;
            write_node_key(f, key)?;
            writeln!(f, ": {:?}", node)?;
        }
        writeln!(f, "  Stale Nodes:")?;
        for (version, stale_batch) in &self.stale_nodes {
            writeln!(f, "v{version}:")?;
            write!(f, "    ")?;
            for stale in stale_batch {
                write_node_key(f, &stale.node_key)?;
                write!(f, ", ")?;
            }
        }
        Ok(())
    }
}
