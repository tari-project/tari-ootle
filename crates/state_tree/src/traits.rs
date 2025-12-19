//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use jmt::{
    storage::{Node, NodeBatch, NodeKey, StaleNodeIndexBatch},
    Version,
};

use crate::{
    diff::{StateTreeNodeBatch, StateTreeStaleNodeIndex, StateTreeStaleNodeIndexBatch},
    StateTreeError,
};

pub trait TreeStoreBatchWriter {
    /// Inserts the node under a new, unique key (i.e. never an update).
    fn batch_insert_nodes(&mut self, nodes: NodeBatch) -> Result<(), StateTreeError>;

    /// Marks the given tree node for a (potential) future removal by an arbitrary external pruning
    /// process.
    fn record_stale_tree_nodes(
        &mut self,
        version: Version,
        stale_nodes: StaleNodeIndexBatch,
    ) -> Result<(), StateTreeError>;
}

/// Implementers are able to insert nodes to a tree store.
pub trait TreeStoreWriter {
    /// Inserts the node under a new, unique key (i.e. never an update).
    fn insert_node(&mut self, key: NodeKey, node: Node) -> Result<(), StateTreeError>;

    /// Marks the given tree part for a (potential) future removal by an arbitrary external pruning
    /// process.
    fn record_stale_tree_node(&mut self, stale_node: StateTreeStaleNodeIndex) -> Result<(), StateTreeError>;
}
