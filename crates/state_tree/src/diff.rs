//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::BTreeMap;

use jmt::{
    storage::{Node, NodeBatch, NodeKey, StaleNodeIndex, TreeUpdateBatch},
    KeyHash,
    OwnedValue,
    Version,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateHashTreeDiff {
    pub new_nodes: StateTreeNodeBatch,
    pub stale_tree_nodes: StateTreeStaleNodeIndexBatch,
}

impl StateHashTreeDiff {
    pub fn new() -> Self {
        Self {
            new_nodes: StateTreeNodeBatch::default(),
            stale_tree_nodes: StateTreeStaleNodeIndexBatch::default(),
        }
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

impl From<StateHashTreeDiff> for NodeBatch {
    fn from(diff: StateHashTreeDiff) -> Self {
        NodeBatch::new(diff.new_nodes.nodes, diff.new_nodes.values)
    }
}

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
