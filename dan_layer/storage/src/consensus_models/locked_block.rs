//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tari_dan_common_types::{Epoch, NodeHeight};

use crate::{
    consensus_models::{Block, BlockId},
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedBlock {
    pub height: NodeHeight,
    pub block_id: BlockId,
    pub epoch: Epoch,
}

impl LockedBlock {
    pub fn height(&self) -> NodeHeight {
        self.height
    }

    pub fn block_id(&self) -> &BlockId {
        &self.block_id
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }
}

impl LockedBlock {
    pub fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.locked_block_get(epoch)
    }

    pub fn get_block<TTx: StateStoreReadTransaction>(&self, tx: &TTx) -> Result<Block, StorageError> {
        tx.blocks_get(&self.block_id)
    }

    pub fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.locked_block_set(self)
    }

    // pub fn get_orphaned_block_ids<TTx: StateStoreReadTransaction>(
    //     &self,
    //     tx: &TTx,
    // ) -> Result<Vec<BlockId>, StorageError> {
    //     tx.blocks_get_orphaned_block_ids(self.block_id())
    // }
    //
    // pub fn remove_orphaned_blocks<TTx>(&self, tx: &mut TTx) -> Result<(), StorageError>
    // where
    //     TTx: StateStoreWriteTransaction + Deref,
    //     TTx::Target: StateStoreReadTransaction,
    // {
    //     let orphaned = self.get_orphaned_block_ids(tx)?;
    //
    //     let other_blocks = Self::get_ids_by_epoch_and_height(&**tx, self.epoch(), self.height())?;
    //     for block_id in other_blocks {
    //         if block_id == *self.id() {
    //             continue;
    //         }
    //         info!(
    //             target: LOG_TARGET,
    //             "❗️🔗 Removing orphaned block {} from epoch {} height {}",
    //             block_id,
    //             self.epoch(),
    //             self.height()
    //         );
    //         crate::consensus_models::block::delete_block_and_children(tx, &block_id)?;
    //     }
    //     Ok(())
    // }
}

impl Display for LockedBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LockedBlock({}, {})", self.height, self.block_id)
    }
}
