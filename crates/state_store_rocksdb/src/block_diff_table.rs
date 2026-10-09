//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The substate changes of blocks that have not yet committed.
//!
//! The table answers every block diff read. Its persistent form is one record per block holding the block's changes in
//! order, written when the block's change set is saved and deleted when the block commits or is orphaned. See
//! [`crate::pending_state`] for how changes are staged and published.

use std::{collections::HashMap, sync::Arc};

use tari_consensus_types::BlockId;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::SubstateVersion;
use tari_ootle_storage::consensus_models::SubstateChange;

/// One block's changes and the indexes that find them. Shared between tables, so staging a table copies none of it.
#[derive(Debug)]
pub(crate) struct BlockDiffEntry {
    /// The block's changes in the order it made them.
    changes: Vec<SubstateChange>,
    by_substate: HashMap<SubstateId, SubstateChanges>,
}

#[derive(Debug, Default)]
struct SubstateChanges {
    /// Position of the substate's last change: its highest version, the DOWN of that version over its UP.
    last: usize,
    /// Positions of each version's UP and DOWN.
    versions: HashMap<SubstateVersion, VersionChanges>,
}

#[derive(Debug, Default)]
struct VersionChanges {
    up: Option<usize>,
    down: Option<usize>,
}

impl VersionChanges {
    /// A version is only ever DOWNed after it is UPed, so its DOWN, if any, is its last change.
    fn last(&self) -> Option<usize> {
        self.down.or(self.up)
    }
}

/// The order in which one substate's changes supersede each other.
pub(crate) fn change_order(change: &SubstateChange) -> (SubstateVersion, bool) {
    (change.versioned_substate_id().version(), !change.is_up())
}

impl BlockDiffEntry {
    pub fn new(changes: Vec<SubstateChange>) -> Self {
        let mut by_substate = HashMap::<SubstateId, SubstateChanges>::new();
        for (position, change) in changes.iter().enumerate() {
            let versioned = change.versioned_substate_id();
            let entry = by_substate
                .entry(versioned.substate_id().clone())
                .or_insert_with(|| SubstateChanges {
                    last: position,
                    versions: HashMap::new(),
                });
            if change_order(&changes[entry.last]) < change_order(change) {
                entry.last = position;
            }
            let version = entry.versions.entry(versioned.version()).or_default();
            let slot = if change.is_up() {
                &mut version.up
            } else {
                &mut version.down
            };
            slot.get_or_insert(position);
        }
        Self { changes, by_substate }
    }

    pub fn changes(&self) -> &[SubstateChange] {
        &self.changes
    }

    /// The block's last change to `substate_id`.
    pub fn last_change(&self, substate_id: &SubstateId) -> Option<&SubstateChange> {
        self.by_substate.get(substate_id).map(|s| &self.changes[s.last])
    }

    /// The block's last change to `version` of `substate_id`.
    pub fn version_change(&self, substate_id: &SubstateId, version: SubstateVersion) -> Option<&SubstateChange> {
        let position = self.by_substate.get(substate_id)?.versions.get(&version)?.last()?;
        Some(&self.changes[position])
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BlockDiffTable {
    blocks: HashMap<BlockId, Arc<BlockDiffEntry>>,
}

impl BlockDiffTable {
    pub fn get(&self, block_id: &BlockId) -> Option<&Arc<BlockDiffEntry>> {
        self.blocks.get(block_id)
    }

    /// Sets the changes `block_id` made, replacing any it made before.
    pub fn insert(&mut self, block_id: BlockId, entry: Arc<BlockDiffEntry>) {
        self.blocks.insert(block_id, entry);
    }

    pub fn remove(&mut self, block_id: &BlockId) -> Option<Arc<BlockDiffEntry>> {
        self.blocks.remove(block_id)
    }
}
