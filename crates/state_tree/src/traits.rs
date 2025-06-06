//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_jellyfish::{JmtStorageError, Node, NodeKey, StaleTreeNode, Version};

pub trait TreeStoreBatchWriter<P> {
    /// Inserts the node under a new, unique key (i.e. never an update).
    fn batch_insert_nodes(&mut self, nodes: Vec<(NodeKey, Node<P>)>) -> Result<(), JmtStorageError>;

    /// Marks the given tree node for a (potential) future removal by an arbitrary external pruning
    /// process.
    fn record_stale_tree_nodes(&mut self, version: Version, nodes: Vec<StaleTreeNode>) -> Result<(), JmtStorageError>;
}
