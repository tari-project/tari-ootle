//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use jmt::{
    storage::{LeafNode, Node, NodeKey, TreeReader, TreeUpdateBatch},
    KeyHash,
    OwnedValue,
    Version,
};

pub struct UpdateBatchStore {
    update: Option<TreeUpdateBatch>,
}

impl UpdateBatchStore {
    pub fn new(update: Option<TreeUpdateBatch>) -> Self {
        Self { update }
    }

    pub fn empty() -> Self {
        Self { update: None }
    }

    pub fn into_update(self) -> Option<TreeUpdateBatch> {
        self.update
    }
}

impl TreeReader for UpdateBatchStore {
    fn get_node_option(&self, node_key: &NodeKey) -> anyhow::Result<Option<Node>> {
        match self.update.as_ref() {
            Some(update) => Ok(update.node_batch.get_node(node_key).cloned()),
            None => Ok(None),
        }
    }

    fn get_value_option(&self, max_version: Version, key_hash: KeyHash) -> anyhow::Result<Option<OwnedValue>> {
        match self.update.as_ref() {
            Some(update) => {
                for ((ver, key), val) in update.node_batch.values().iter().rev() {
                    if *ver <= max_version && *key == key_hash {
                        return Ok(val.clone());
                    }
                }
                Ok(None)
            },
            None => Ok(None),
        }
    }

    fn get_rightmost_leaf(&self) -> anyhow::Result<Option<(NodeKey, LeafNode)>> {
        match self.update.as_ref() {
            Some(update) => {
                for (key, node) in update.node_batch.nodes().iter().rev() {
                    if let Node::Leaf(leaf) = node {
                        return Ok(Some((key.clone(), leaf.clone())));
                    }
                }
                Ok(None)
            },
            None => Ok(None),
        }
    }
}
