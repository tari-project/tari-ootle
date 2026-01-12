//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{collections::BTreeMap, fmt::Display};

use jmt::{
    storage::{Node, NodeBatch, NodeKey, StaleNodeIndex, TreeUpdateBatch},
    KeyHash,
    OwnedValue,
    Version,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateHashTreeDiff {
    pub new_nodes: StateTreeNodeBatch,
    pub stale_tree_nodes: StateTreeStaleNodeIndexBatch,
}

impl StateHashTreeDiff {
    pub fn new() -> Self {
        Self::default()
    }
}

impl From<TreeUpdateBatch> for StateHashTreeDiff {
    fn from(batch: TreeUpdateBatch) -> Self {
        Self {
            new_nodes: batch.node_batch.into(),
            stale_tree_nodes: batch.stale_node_index_batch.into_iter().map(Into::into).collect(),
        }
    }
}

pub type NodeValue = Box<[u8]>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StateTreeNodeBatch {
    pub nodes: BTreeMap<NodeKey, Node>,
    pub values: BTreeMap<(Version, KeyHash), Option<OwnedValue>>,
}

impl From<NodeBatch> for StateTreeNodeBatch {
    fn from(batch: NodeBatch) -> Self {
        Self {
            nodes: batch.nodes().iter().map(|(k, n)| (k.clone(), n.clone())).collect(),
            values: batch.values().iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        }
    }
}

impl From<StateTreeNodeBatch> for NodeBatch {
    fn from(batch: StateTreeNodeBatch) -> Self {
        Self::new(batch.nodes, batch.values)
    }
}

impl Display for StateTreeNodeBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "NodeBatch {{ nodes: {}, values: {} }}",
            self.nodes.len(),
            self.values.len()
        )
    }
}

pub type StateTreeStaleNodeIndexBatch = Vec<StateTreeStaleNodeIndex>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTreeStaleNodeIndex {
    /// The version since when the node is overwritten and becomes stale.
    pub stale_since_version: Version,
    /// The [`NodeKey`](node_type/struct.NodeKey.html) identifying the node associated with this
    /// record.
    pub node_key: NodeKey,
}

impl From<StaleNodeIndex> for StateTreeStaleNodeIndex {
    fn from(idx: StaleNodeIndex) -> Self {
        Self {
            stale_since_version: idx.stale_since_version,
            node_key: idx.node_key,
        }
    }
}
impl From<StateTreeStaleNodeIndex> for StaleNodeIndex {
    fn from(idx: StateTreeStaleNodeIndex) -> Self {
        Self {
            stale_since_version: idx.stale_since_version,
            node_key: idx.node_key,
        }
    }
}

pub struct DisplayNodeKey<'a> {
    node_key: &'a NodeKey,
}

impl Display for DisplayNodeKey<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NodeKey(v{}, ", self.node_key.version())?;
        for n in self.node_key.nibble_path().nibbles() {
            let nibble = u8::from(n);
            // Single hex char since nibble is 0-15
            write!(f, "{:x}", nibble)?;
        }
        write!(f, ")")?;
        Ok(())
    }
}

pub fn display_node_key(node_key: &NodeKey) -> DisplayNodeKey<'_> {
    DisplayNodeKey { node_key }
}
