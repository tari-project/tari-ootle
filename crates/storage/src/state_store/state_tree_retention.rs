//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
};

use tari_ootle_common_types::{Epoch, NumPreshards, ShardGroup, optional::Optional, shard::Shard};
use tari_state_tree::Version;

use crate::{StateStoreReadTransaction, StorageError};

/// The trees the stale state tree node GC keeps beyond the state history window: each shard's whole tree at each of
/// its pinned versions. Input to [`crate::StateStoreWriteTransaction::state_tree_nodes_clear_stale`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateTreeRetention {
    shards: BTreeMap<Shard, ShardTreeRetention>,
}

impl StateTreeRetention {
    /// Each shard's latest version, with its version in each of the last `pinned_checkpoint_epochs` epoch checkpoints
    /// stored pinned.
    ///
    /// The latest versions are read first: a checkpoint is stored before any version above it commits, so every
    /// checkpoint at a version below a shard's latest is among those read after it.
    pub fn read<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        num_preshards: NumPreshards,
        pinned_checkpoint_epochs: u64,
    ) -> Result<Self, StorageError> {
        let mut retention = Self::default();
        for shard in iter::once(Shard::global()).chain(ShardGroup::all_shards(num_preshards).shard_iter()) {
            if let Some(version) = tx.state_tree_versions_get_latest(shard)? {
                retention.add_shard(shard, version);
            }
        }
        let Some(oldest_pinned) = pinned_checkpoint_epochs.checked_sub(1) else {
            return Ok(retention);
        };
        let Some(last) = tx.epoch_checkpoint_get_last().optional()? else {
            return Ok(retention);
        };
        let from_epoch = last.epoch().saturating_sub(Epoch(oldest_pinned));
        for checkpoint in tx.epoch_checkpoint_get_all_from_epoch(from_epoch, usize::MAX)? {
            for (shard, summary) in checkpoint.shard_tree_summary() {
                retention.pin(*shard, summary.state_version);
            }
        }
        Ok(retention)
    }

    /// Adds `shard`, whose latest version is `latest_version`, with no pinned version.
    pub fn add_shard(&mut self, shard: Shard, latest_version: Version) -> &mut Self {
        self.shards.insert(shard, ShardTreeRetention {
            latest_version,
            pinned_versions: BTreeSet::new(),
        });
        self
    }

    /// Pins the tree of `shard` at `version`. A shard not added has no tree to keep, so its pins are ignored.
    pub fn pin(&mut self, shard: Shard, version: Version) -> &mut Self {
        if let Some(retention) = self.shards.get_mut(&shard) {
            retention.pinned_versions.insert(version);
        }
        self
    }

    pub fn get(&self, shard: Shard) -> Option<&ShardTreeRetention> {
        self.shards.get(&shard)
    }

    /// Each shard added and what is kept of its tree, in shard order.
    pub fn iter(&self) -> impl Iterator<Item = (Shard, &ShardTreeRetention)> {
        self.shards.iter().map(|(shard, retention)| (*shard, retention))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShardTreeRetention {
    /// The shard's latest version, read before its pins. The history window is measured back from it.
    ///
    /// Pins are the versions epoch checkpoints record, and a checkpoint is stored before any version above it
    /// commits. Every version the GC filters lies below this one, so every pin it must honour was already stored
    /// when the pins were read.
    pub latest_version: Version,
    pub pinned_versions: BTreeSet<Version>,
}

impl ShardTreeRetention {
    /// The highest pinned version whose tree holds a node created at `created` and made stale at `stale_at`, if any.
    /// The node belongs to the tree of every version from `created` up to, but excluding, `stale_at`.
    pub fn pin_holding(&self, created: Version, stale_at: Version) -> Option<Version> {
        if created >= stale_at {
            return None;
        }
        self.pinned_versions.range(created..stale_at).next_back().copied()
    }

    /// The lowest pinned version. A node kept only for a pin below it is no longer needed.
    pub fn lowest_pin(&self) -> Option<Version> {
        self.pinned_versions.first().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retention(pins: &[Version]) -> ShardTreeRetention {
        ShardTreeRetention {
            latest_version: 1_000,
            pinned_versions: pins.iter().copied().collect(),
        }
    }

    #[test]
    fn a_node_is_held_by_the_highest_pin_in_its_lifetime() {
        let retention = retention(&[10, 20, 30]);
        assert_eq!(retention.pin_holding(5, 25), Some(20));
        assert_eq!(retention.pin_holding(10, 31), Some(30));
        assert_eq!(retention.pin_holding(20, 21), Some(20));
    }

    #[test]
    fn a_node_stale_at_or_created_after_a_pin_is_not_held_by_it() {
        let retention = retention(&[10, 20]);
        assert_eq!(retention.pin_holding(11, 20), None);
        assert_eq!(retention.pin_holding(1, 10), None);
        assert_eq!(retention.pin_holding(21, 40), None);
        assert_eq!(retention.pin_holding(15, 15), None);
    }

    #[test]
    fn pins_of_a_shard_not_added_are_ignored() {
        let mut retention = StateTreeRetention::default();
        retention.add_shard(Shard::from_u32(1), 50).pin(Shard::from_u32(1), 40);
        retention.pin(Shard::from_u32(2), 40);
        assert_eq!(retention.get(Shard::from_u32(1)).unwrap().lowest_pin(), Some(40));
        assert!(retention.get(Shard::from_u32(2)).is_none());
    }
}
