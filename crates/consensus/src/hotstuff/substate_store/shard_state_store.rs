//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::ops::Deref;

use log::*;
use tari_ootle_common_types::{optional::Optional, shard::Shard};
use tari_ootle_storage::{StateStoreReadTransaction, StateStoreWriteTransaction};
use tari_state_tree::{
    JmtStorageError,
    Node,
    NodeKey,
    StaleTreeNode,
    StateTreePayload,
    TreeStoreBatchWriter,
    TreeStoreReader,
    Version,
};

const LOG_TARGET: &str = "tari::ootle::consensus::sharded_state_tree";

/// Tree store that is scoped to a specific shard
#[derive(Debug)]
pub struct ShardScopedTreeStoreReader<'a, TTx> {
    shard: Shard,
    tx: &'a TTx,
}

impl<'a, TTx> ShardScopedTreeStoreReader<'a, TTx> {
    pub fn new(tx: &'a TTx, shard: Shard) -> Self {
        Self { shard, tx }
    }
}

impl<TTx: StateStoreReadTransaction> TreeStoreReader<StateTreePayload> for ShardScopedTreeStoreReader<'_, TTx> {
    fn get_node(&self, key: &NodeKey) -> Result<Node<StateTreePayload>, tari_state_tree::JmtStorageError> {
        self.tx
            .state_tree_nodes_get(self.shard, key)
            .optional()
            .map_err(|e| tari_state_tree::JmtStorageError::UnexpectedError(e.to_string()))?
            .ok_or_else(|| {
                warn!(
                    target: LOG_TARGET,
                    "ShardScopedTreeStoreReader: Node not found in shard {} with key: {}", self.shard, key
                );
                tari_state_tree::JmtStorageError::NotFound(key.clone())
            })
    }
}

#[derive(Debug)]
pub struct ShardScopedTreeStoreWriter<'a, TTx> {
    shard: Shard,
    tx: &'a mut TTx,
}

impl<'a, TTx: StateStoreWriteTransaction> ShardScopedTreeStoreWriter<'a, TTx> {
    pub fn new(tx: &'a mut TTx, shard: Shard) -> Self {
        Self { shard, tx }
    }

    pub fn set_state_version(&mut self, version: Version) -> Result<(), tari_state_tree::JmtStorageError> {
        self.tx
            .state_tree_shard_versions_set(self.shard, version)
            .map_err(|e| tari_state_tree::JmtStorageError::UnexpectedError(e.to_string()))
    }

    pub fn record_stale_tree_nodes(
        &mut self,
        version: Version,
        nodes: Vec<StaleTreeNode>,
    ) -> Result<(), tari_state_tree::JmtStorageError> {
        self.tx
            .state_tree_nodes_record_stale_tree_nodes(self.shard, version, nodes)
            .map_err(|e| tari_state_tree::JmtStorageError::UnexpectedError(e.to_string()))
    }

    pub fn insert_nodes(
        &mut self,
        nodes: Vec<(NodeKey, Node<StateTreePayload>)>,
    ) -> Result<(), tari_state_tree::JmtStorageError> {
        self.tx
            .state_tree_nodes_batch_insert(self.shard, nodes)
            .map_err(|e| tari_state_tree::JmtStorageError::UnexpectedError(e.to_string()))
    }

    pub fn transaction(&mut self) -> &mut TTx {
        self.tx
    }
}

impl<TTx> TreeStoreReader<StateTreePayload> for ShardScopedTreeStoreWriter<'_, TTx>
where
    TTx: StateStoreWriteTransaction + Deref,
    TTx::Target: StateStoreReadTransaction,
{
    fn get_node(&self, key: &NodeKey) -> Result<Node<StateTreePayload>, tari_state_tree::JmtStorageError> {
        self.tx
            .state_tree_nodes_get(self.shard, key)
            .optional()
            .map_err(|e| tari_state_tree::JmtStorageError::UnexpectedError(e.to_string()))?
            .ok_or_else(|| {
                warn!(
                    target: LOG_TARGET,
                    "ShardScopedTreeStoreWriter: Node not found in shard {} with key: {}", self.shard, key
                );
                tari_state_tree::JmtStorageError::NotFound(key.clone())
            })
    }
}

impl<TTx: StateStoreWriteTransaction> TreeStoreBatchWriter<StateTreePayload> for ShardScopedTreeStoreWriter<'_, TTx> {
    fn batch_insert_nodes(&mut self, nodes: Vec<(NodeKey, Node<StateTreePayload>)>) -> Result<(), JmtStorageError> {
        self.tx
            .state_tree_nodes_batch_insert(self.shard, nodes)
            .map_err(|e| tari_state_tree::JmtStorageError::UnexpectedError(e.to_string()))
    }

    fn record_stale_tree_nodes(&mut self, version: Version, nodes: Vec<StaleTreeNode>) -> Result<(), JmtStorageError> {
        self.tx
            .state_tree_nodes_record_stale_tree_nodes(self.shard, version, nodes)
            .map_err(|e| tari_state_tree::JmtStorageError::UnexpectedError(e.to_string()))
    }
}
