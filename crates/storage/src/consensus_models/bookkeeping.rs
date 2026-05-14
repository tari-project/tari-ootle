//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::{
    HighPc,
    HighTc,
    HighestSeenBlock,
    LastExecuted,
    LastProposed,
    LastSentNewView,
    LastSentVote,
    LastVoted,
    LeafBlock,
    LockedBlock,
};
use tari_ootle_common_types::Epoch;

use crate::{StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};

pub trait BookkeepingModel: Sized {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError>;
    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError>;
}

/// Helpers for reading bookkeeping records without specifying an epoch up-front.
/// Used by recovery paths that need to discover the consensus epoch from disk.
pub trait BookkeepingEpochAgnosticRead: Sized {
    fn get_any<TTx: StateStoreReadTransaction>(tx: &TTx) -> Result<Self, StorageError>;
}

impl BookkeepingEpochAgnosticRead for LeafBlock {
    fn get_any<TTx: StateStoreReadTransaction>(tx: &TTx) -> Result<Self, StorageError> {
        tx.leaf_block_get_any()
    }
}

impl BookkeepingEpochAgnosticRead for HighPc {
    fn get_any<TTx: StateStoreReadTransaction>(tx: &TTx) -> Result<Self, StorageError> {
        tx.high_pc_get_any()
    }
}

impl BookkeepingModel for HighPc {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.high_pc_get(epoch)
    }

    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.high_pc_set(self)
    }
}

impl BookkeepingModel for HighTc {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.high_tc_get(epoch)
    }

    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.high_tc_set(self)
    }
}

impl BookkeepingModel for LastExecuted {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.last_executed_get(epoch)
    }

    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.last_executed_set(self)
    }
}

impl BookkeepingModel for LastVoted {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.last_voted_get(epoch)
    }

    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.last_voted_set(self)
    }
}

impl BookkeepingModel for LeafBlock {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.leaf_block_get(epoch)
    }

    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.leaf_block_set(self)
    }
}

impl BookkeepingModel for HighestSeenBlock {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.highest_seen_block_get(epoch)
    }

    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.highest_seen_block_set(self)
    }
}

impl BookkeepingModel for LockedBlock {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.locked_block_get(epoch)
    }

    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.locked_block_set(self)
    }
}

impl BookkeepingModel for LastProposed {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.last_proposed_get(epoch)
    }

    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.last_proposed_set(self)
    }
}

impl BookkeepingModel for LastSentVote {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.last_sent_vote_get(epoch)
    }

    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.last_sent_vote_set(self)
    }
}

impl BookkeepingModel for LastSentNewView {
    fn get<TTx: StateStoreReadTransaction>(tx: &TTx, epoch: Epoch) -> Result<Self, StorageError> {
        tx.last_sent_new_view_get(epoch)
    }

    fn set<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.last_sent_new_view_set(self)
    }
}
