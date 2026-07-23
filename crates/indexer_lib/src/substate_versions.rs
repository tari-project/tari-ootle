//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, mem, sync::Mutex};

use tari_engine_types::substate::SubstateId;

/// Number of substate IDs held per generation. Two generations are kept, so the tracker remembers
/// between one and two times this many of the most recently updated substates.
const CAPACITY_PER_GENERATION: usize = 50_000;

/// The newest version of each recently-updated substate, as observed from committed transaction
/// receipts.
///
/// A substate cache entry records the version that was current when it was written, and nothing
/// touches that entry when the substate is later spent, so an entry cannot on its own distinguish
/// "current" from "superseded". This tracker supplies that missing signal: a committed receipt names
/// every substate the transaction upped, which is exactly the point at which the previous version of
/// each went down.
///
/// Capacity is bounded, so this is a positive signal only: [`Self::is_superseded`] returning `false`
/// means "not known to be superseded", never "current".
#[derive(Debug)]
pub struct SubstateVersionTracker {
    inner: Mutex<Generations>,
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
            inner: Mutex::new(Generations::default()),
            capacity_per_generation: capacity_per_generation.max(1),
        }
    }

    /// Records that `substate_id` reached `version`, implying every lower version of it is down.
    pub fn record_up(&self, substate_id: &SubstateId, version: u32) {
        let mut inner = self.lock();
        if let Some(recorded) = inner.current.get_mut(substate_id) {
            *recorded = (*recorded).max(version);
            return;
        }
        inner.current.insert(substate_id.clone(), version);

        // Roll generations rather than evicting individually: a substate that is still being updated
        // is re-recorded into the new generation on its next commit, while everything untouched ages
        // out. Lookups continue to hit the previous generation until it is dropped in turn.
        if inner.current.len() > self.capacity_per_generation {
            inner.previous = mem::take(&mut inner.current);
        }
    }

    /// True if a version newer than `version` of `substate_id` is known to have been committed.
    pub fn is_superseded(&self, substate_id: &SubstateId, version: u32) -> bool {
        let inner = self.lock();
        let current = inner.current.get(substate_id).copied();
        let previous = inner.previous.get(substate_id).copied();
        current.max(previous).is_some_and(|latest| latest > version)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Generations> {
        // The guarded state is a plain map with no invariants to uphold across a panic, so a poisoned
        // lock is recovered rather than propagated.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
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
    fn updated_substates_survive_generation_rolls() {
        let tracker = SubstateVersionTracker::with_capacity(2);
        for round in 1..=10u8 {
            tracker.record_up(&component(1), u32::from(round));
            tracker.record_up(&component(round + 1), 1);
        }
        assert!(tracker.is_superseded(&component(1), 9));
    }
}
