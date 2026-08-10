//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    mem,
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use tari_engine_types::substate::SubstateId;

/// Substate IDs held per generation. Two are kept, so the tracker remembers between one and two times
/// this many of the most recently updated substates.
const CAPACITY_PER_GENERATION: usize = 50_000;

/// The newest version of each recently-updated substate, taken from committed transaction receipts.
///
/// A substate cache entry holds the version that was current when it was written and is not touched
/// when that version is later spent, so it cannot tell "current" from "superseded" on its own. This
/// supplies that signal. Capacity is bounded, so it is positive-only: [`Self::is_superseded`]
/// returning `false` means "not known to be superseded", never "current".
#[derive(Debug)]
pub struct SubstateVersionTracker {
    inner: RwLock<Generations>,
    capacity_per_generation: usize,
}

#[derive(Debug, Default)]
struct Generations {
    current: HashMap<SubstateId, u32>,
    previous: HashMap<SubstateId, u32>,
}

impl SubstateVersionTracker {
    pub fn new() -> Self {
        Self::with_capacity(CAPACITY_PER_GENERATION)
    }

    pub fn with_capacity(capacity_per_generation: usize) -> Self {
        Self {
            inner: RwLock::new(Generations::default()),
            capacity_per_generation: capacity_per_generation.max(1),
        }
    }

    /// Records that `substate_id` reached `version`, implying every lower version of it is down.
    pub fn record_up(&self, substate_id: &SubstateId, version: u32) {
        self.record_up_all(std::iter::once((substate_id, version)));
    }

    /// Batched form of [`Self::record_up`], taking the write lock once for the whole batch rather than
    /// contending with concurrent lookups on every substate.
    pub fn record_up_all<'a, I>(&self, ups: I)
    where I: IntoIterator<Item = (&'a SubstateId, u32)> {
        let mut inner = self.write();
        for (substate_id, version) in ups {
            if let Some(recorded) = inner.current.get_mut(substate_id) {
                *recorded = (*recorded).max(version);
                continue;
            }
            inner.current.insert(substate_id.clone(), version);

            // Rolling rather than evicting individually keeps substates that are still being updated:
            // they are re-recorded into the new generation on their next commit.
            if inner.current.len() > self.capacity_per_generation {
                inner.previous = mem::take(&mut inner.current);
            }
        }
    }

    /// True if a version newer than `version` of `substate_id` is known to have been committed.
    pub fn is_superseded(&self, substate_id: &SubstateId, version: u32) -> bool {
        let inner = self.read();
        let current = inner.current.get(substate_id).copied();
        let previous = inner.previous.get(substate_id).copied();
        current.max(previous).is_some_and(|latest| latest > version)
    }

    // Poisoning is recovered from rather than propagated: the maps hold no invariant a panic could
    // break.

    fn read(&self) -> RwLockReadGuard<'_, Generations> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Generations> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for SubstateVersionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(n: u8) -> SubstateId {
        format!("component_{:064x}", n).parse().unwrap()
    }

    #[test]
    fn unknown_substate_is_not_superseded() {
        let tracker = SubstateVersionTracker::new();
        assert!(!tracker.is_superseded(&component(1), 0));
    }

    #[test]
    fn only_older_versions_are_superseded() {
        let tracker = SubstateVersionTracker::new();
        tracker.record_up(&component(1), 5);
        assert!(tracker.is_superseded(&component(1), 4));
        assert!(!tracker.is_superseded(&component(1), 5));
        assert!(!tracker.is_superseded(&component(1), 6));
    }

    #[test]
    fn out_of_order_records_keep_the_newest_version() {
        let tracker = SubstateVersionTracker::new();
        tracker.record_up(&component(1), 5);
        tracker.record_up(&component(1), 3);
        assert!(tracker.is_superseded(&component(1), 4));
    }

    #[test]
    fn previous_generation_is_still_consulted() {
        let tracker = SubstateVersionTracker::with_capacity(2);
        tracker.record_up(&component(1), 5);
        for i in 2..=5 {
            tracker.record_up(&component(i), 1);
        }
        assert!(tracker.is_superseded(&component(1), 4));
    }

    #[test]
    fn batch_records_every_substate_at_its_newest_version() {
        let tracker = SubstateVersionTracker::new();
        let ids = (0..=u8::MAX).map(component).collect::<Vec<_>>();
        // Every id appears twice, newest first, so the batch covers the duplicate-within-a-batch path.
        let batch = ids.iter().map(|id| (id, 7)).chain(ids.iter().map(|id| (id, 3)));
        tracker.record_up_all(batch);
        for id in &ids {
            assert!(tracker.is_superseded(id, 6));
            assert!(!tracker.is_superseded(id, 7));
        }
    }

    #[test]
    fn updated_substates_survive_generation_rolls() {
        let tracker = SubstateVersionTracker::with_capacity(2);
        for round in 1..=10u8 {
            tracker.record_up(&component(1), u32::from(round));
            tracker.record_up(&component(round + 1), 1);
        }
        assert!(tracker.is_superseded(&component(1), 9));
    }
}
