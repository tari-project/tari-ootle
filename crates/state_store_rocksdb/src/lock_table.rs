//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The substate locks held by blocks whose transactions have not yet released them.
//!
//! The table answers every lock read. Its persistent form is one [`BlockLockSet`] record per block, and a record always
//! holds exactly the block's unreleased locks: it is rewritten when some of them are released and deleted when the last
//! one is. See [`crate::pending_state`] for how changes are staged and published.

use std::collections::HashMap;

use indexmap::IndexMap;
use tari_consensus_types::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::consensus_models::SubstateLock;
use tari_ootle_transaction::TransactionId;

use crate::column_families::substate_locks::{BlockLockSet, SubstateLockGrants};

/// The locks one block granted that are still held.
#[derive(Debug, Clone)]
pub(crate) struct BlockLocks {
    pub epoch: Epoch,
    pub height: NodeHeight,
    /// Each substate's locks in the order the block granted them.
    pub substates: IndexMap<SubstateId, Vec<SubstateLock>>,
}

impl BlockLocks {
    pub fn to_record(&self) -> BlockLockSet {
        BlockLockSet {
            block_epoch: self.epoch,
            block_height: self.height,
            substates: self
                .substates
                .iter()
                .map(|(substate_id, locks)| SubstateLockGrants {
                    substate_id: substate_id.clone(),
                    locks: locks.clone(),
                })
                .collect(),
        }
    }

    pub fn from_record(record: BlockLockSet) -> Self {
        Self {
            epoch: record.block_epoch,
            height: record.block_height,
            substates: record
                .substates
                .into_iter()
                .map(|grants| (grants.substate_id, grants.locks))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LockTable {
    blocks: HashMap<BlockId, BlockLocks>,
    /// The blocks holding a lock on each substate.
    by_substate: HashMap<SubstateId, Vec<BlockId>>,
    /// The (block, substate) pairs each transaction holds a lock on.
    by_transaction: HashMap<TransactionId, Vec<(BlockId, SubstateId)>>,
}

impl LockTable {
    pub fn block(&self, block_id: &BlockId) -> Option<&BlockLocks> {
        self.blocks.get(block_id)
    }

    pub fn holds_locks(&self, transaction_id: &TransactionId) -> bool {
        self.by_transaction.contains_key(transaction_id)
    }

    /// The blocks holding a lock on `substate_id`, with those locks in grant order.
    pub fn blocks_locking_substate<'a>(
        &'a self,
        substate_id: &'a SubstateId,
    ) -> impl Iterator<Item = (&'a BlockId, &'a BlockLocks, &'a [SubstateLock])> + 'a {
        self.by_substate
            .get(substate_id)
            .into_iter()
            .flatten()
            .filter_map(move |block_id| {
                let block = self.blocks.get(block_id)?;
                let locks = block.substates.get(substate_id)?;
                Some((block_id, block, locks.as_slice()))
            })
    }

    /// Every lock `transaction_id` holds, with the block that granted it and its position in the block's grant order
    /// for the substate.
    pub fn locks_held_by<'a>(
        &'a self,
        transaction_id: &'a TransactionId,
    ) -> impl Iterator<Item = (&'a BlockId, &'a BlockLocks, &'a SubstateId, usize, &'a SubstateLock)> + 'a {
        self.by_transaction
            .get(transaction_id)
            .into_iter()
            .flatten()
            .filter_map(move |(block_id, substate_id)| {
                let block = self.blocks.get(block_id)?;
                let (substate_id, locks) = block.substates.get_key_value(substate_id)?;
                Some((block_id, block, substate_id, locks))
            })
            .flat_map(move |(block_id, block, substate_id, locks)| {
                locks
                    .iter()
                    .enumerate()
                    .filter(move |(_, lock)| lock.transaction_id() == transaction_id)
                    .map(move |(grant_seq, lock)| (block_id, block, substate_id, grant_seq, lock))
            })
    }

    /// Sets the locks `block_id` holds, replacing any it held before.
    pub fn insert_block(&mut self, block_id: BlockId, locks: BlockLocks) {
        self.remove_block(&block_id);
        for (substate_id, substate_locks) in &locks.substates {
            push_unique(self.by_substate.entry(substate_id.clone()).or_default(), block_id);
            for lock in substate_locks {
                push_unique(
                    self.by_transaction.entry(*lock.transaction_id()).or_default(),
                    (block_id, substate_id.clone()),
                );
            }
        }
        self.blocks.insert(block_id, locks);
    }

    /// Removes every lock `block_id` holds. Returns false if it held none.
    pub fn remove_block(&mut self, block_id: &BlockId) -> bool {
        let Some(block) = self.blocks.remove(block_id) else {
            return false;
        };
        for (substate_id, locks) in block.substates {
            remove_from_index(&mut self.by_substate, &substate_id, |id| id == block_id);
            for lock in locks {
                remove_from_index(&mut self.by_transaction, lock.transaction_id(), |(id, _)| {
                    id == block_id
                });
            }
        }
        true
    }

    /// Releases every lock the transactions hold. Returns the blocks that held any of them, each once; a block left
    /// holding no locks is removed from the table.
    pub fn release<'a, I: IntoIterator<Item = &'a TransactionId>>(&mut self, transaction_ids: I) -> Vec<BlockId> {
        let mut affected = Vec::new();
        for transaction_id in transaction_ids {
            let Some(held) = self.by_transaction.remove(transaction_id) else {
                continue;
            };
            for (block_id, substate_id) in held {
                let Some(block) = self.blocks.get_mut(&block_id) else {
                    continue;
                };
                push_unique(&mut affected, block_id);
                let Some(locks) = block.substates.get_mut(&substate_id) else {
                    continue;
                };
                locks.retain(|lock| lock.transaction_id() != transaction_id);
                if locks.is_empty() {
                    block.substates.shift_remove(&substate_id);
                    remove_from_index(&mut self.by_substate, &substate_id, |id| *id == block_id);
                }
                if block.substates.is_empty() {
                    self.blocks.remove(&block_id);
                }
            }
        }
        affected
    }
}

fn push_unique<T: PartialEq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}

fn remove_from_index<K, V, F>(index: &mut HashMap<K, Vec<V>>, key: &K, mut matches: F)
where
    K: std::hash::Hash + Eq,
    F: FnMut(&V) -> bool,
{
    if let Some(entries) = index.get_mut(key) {
        entries.retain(|v| !matches(v));
        if entries.is_empty() {
            index.remove(key);
        }
    }
}
