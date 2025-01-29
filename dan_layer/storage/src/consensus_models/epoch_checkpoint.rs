//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use indexmap::IndexMap;
use tari_dan_common_types::{shard::Shard, Epoch};
use tari_state_tree::{compute_merkle_root_for_hashes, StateTreeError, TreeHash, SPARSE_MERKLE_PLACEHOLDER_HASH};

use crate::{
    consensus_models::{Block, QuorumCertificate},
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};

#[derive(Debug, Clone)]
pub struct EpochCheckpoint {
    block: Block,
    linked_qcs: Vec<QuorumCertificate>,
    shard_roots: IndexMap<Shard, TreeHash>,
}

impl EpochCheckpoint {
    pub fn new(block: Block, linked_qcs: Vec<QuorumCertificate>, shard_roots: IndexMap<Shard, TreeHash>) -> Self {
        Self {
            block,
            linked_qcs,
            shard_roots,
        }
    }

    pub fn qcs(&self) -> &[QuorumCertificate] {
        &self.linked_qcs
    }

    pub fn block(&self) -> &Block {
        &self.block
    }

    pub fn shard_roots(&self) -> &IndexMap<Shard, TreeHash> {
        &self.shard_roots
    }

    pub fn get_shard_root(&self, shard: Shard) -> TreeHash {
        self.shard_roots
            .get(&shard)
            .copied()
            .unwrap_or(SPARSE_MERKLE_PLACEHOLDER_HASH)
    }

    pub fn compute_state_merkle_root(&self) -> Result<TreeHash, StateTreeError> {
        let shard_group = self.block().shard_group();
        let hashes = shard_group.shard_iter().map(|shard| self.get_shard_root(shard));
        compute_merkle_root_for_hashes(hashes)
    }
}

impl EpochCheckpoint {
    pub fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.epoch_checkpoint_get(epoch)
    }

    pub fn save<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.epoch_checkpoint_save(self)
    }
}

impl Display for EpochCheckpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EpochCheckpoint: block={}, qcs=", self.block)?;
        for qc in self.qcs() {
            write!(f, "{}, ", qc.id())?;
        }
        Ok(())
    }
}
