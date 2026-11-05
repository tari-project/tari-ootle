//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tari_ootle_common_types::{Epoch, NodeHeight, ShardGroup, StateVersion, shard::Shard};
use tari_ootle_p2p::PeerAddress;
use tari_state_tree::LeafKey;

/// The width of one bucket of promotion activity.
pub const ACTIVITY_BUCKET: Duration = Duration::from_secs(10);
/// How many buckets of promotion activity are kept: ten minutes of them.
const ACTIVITY_BUCKETS: usize = 60;
/// How many of a shard group's most recent blames are kept.
const MAX_BLAMES: usize = 8;

/// What each shard group's sync loop is doing and what it last saw. Diagnostic only: the sync writes it and the REST
/// API reads it.
#[derive(Debug, Clone, Default)]
pub struct SyncStatusMonitor {
    inner: Arc<Mutex<SyncStatus>>,
}

#[derive(Debug, Clone, Default)]
pub struct SyncStatus {
    pub groups: BTreeMap<ShardGroup, GroupStatus>,
    pub shards: BTreeMap<Shard, ShardStatus>,
}

#[derive(Debug, Clone)]
pub struct GroupStatus {
    pub phase: GroupPhase,
    pub phase_since: SystemTime,
    /// The committee member the group's current or last round opened its session with.
    pub stream_peer: Option<PeerAddress>,
    /// The most recent blames, oldest first.
    pub blamed: VecDeque<Blame>,
    /// Every peer blamed during an epoch, which is excluded from the group for the rest of it, and that epoch.
    pub excluded: (Epoch, BTreeSet<PeerAddress>),
    pub tip: Option<ObservedTip>,
    /// The epoch of the last checkpoint the group stored, and when it was stored.
    pub last_checkpoint: Option<(Epoch, SystemTime)>,
}

impl GroupStatus {
    fn new(now: SystemTime) -> Self {
        Self {
            phase: GroupPhase::Starting,
            phase_since: now,
            stream_peer: None,
            blamed: VecDeque::new(),
            excluded: (Epoch(0), BTreeSet::new()),
            tip: None,
            last_checkpoint: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupPhase {
    Starting,
    SyncingCheckpoints,
    Snapshotting {
        shard: Shard,
    },
    Streaming,
    Paused {
        reason: &'static str,
        until: SystemTime,
    },
    /// Every committee member's history of `shard` mismatched from the version after `from`, so the group stops
    /// following until the epoch moves.
    Halted {
        shard: Shard,
        from: Option<StateVersion>,
    },
}

#[derive(Debug, Clone)]
pub struct Blame {
    pub peers: Vec<PeerAddress>,
    pub epoch: Epoch,
    pub at: SystemTime,
    pub details: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedTip {
    pub epoch: Epoch,
    pub height: NodeHeight,
    pub observed_at: SystemTime,
}

#[derive(Debug, Clone, Default)]
pub struct ShardStatus {
    /// The latest verified leaf a tip probe returned for the shard, and when.
    pub tip_leaf: Option<(StateVersion, SystemTime)>,
    pub last_promoted_at: Option<SystemTime>,
    pub last_mismatch: Option<(SystemTime, String)>,
    pub snapshot: Option<SnapshotProgress>,
    pub promoted: ActivityWindow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapshotProgress {
    pub state_version: StateVersion,
    pub substates: usize,
    /// How far through the leaf key space the snapshot has paged. Leaf keys are hashes, so this tracks the share of
    /// the shard's substates received.
    pub key_fraction: f64,
    pub started_at: SystemTime,
}

/// What was promoted in each [`ACTIVITY_BUCKET`] of the last [`ACTIVITY_BUCKETS`].
#[derive(Debug, Clone, Default)]
pub struct ActivityWindow {
    /// Ascending by start, holding only the buckets something was promoted in.
    buckets: VecDeque<ActivityBucket>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActivityBucket {
    /// Unix seconds, a multiple of [`ACTIVITY_BUCKET`].
    pub start: u64,
    pub versions: u64,
    pub transactions: u64,
}

impl ActivityWindow {
    pub fn record(&mut self, now: SystemTime, versions: u64, transactions: u64) {
        let start = bucket_start(now);
        match self.buckets.back_mut() {
            Some(last) if last.start == start => {
                last.versions += versions;
                last.transactions += transactions;
            },
            _ => self.buckets.push_back(ActivityBucket {
                start,
                versions,
                transactions,
            }),
        }
        self.expire(start);
    }

    /// Every bucket of the window ending at `now`, oldest first, including those nothing was promoted in.
    pub fn buckets(&self, now: SystemTime) -> Vec<ActivityBucket> {
        let last = bucket_start(now);
        let first = first_bucket(last);
        let mut recorded = self.buckets.iter().filter(|b| b.start >= first).peekable();
        (0..ACTIVITY_BUCKETS as u64)
            .map(|i| {
                let start = first + i * ACTIVITY_BUCKET.as_secs();
                match recorded.next_if(|b| b.start == start) {
                    Some(bucket) => *bucket,
                    None => ActivityBucket {
                        start,
                        ..Default::default()
                    },
                }
            })
            .collect()
    }

    /// The versions promoted in the buckets of the last `span` ending at `now`.
    pub fn versions_within(&self, now: SystemTime, span: Duration) -> u64 {
        let from = bucket_start(now).saturating_sub(span.as_secs().saturating_sub(ACTIVITY_BUCKET.as_secs()));
        self.buckets
            .iter()
            .filter(|b| b.start >= from)
            .map(|b| b.versions)
            .sum()
    }

    fn expire(&mut self, last: u64) {
        let first = first_bucket(last);
        while self.buckets.front().is_some_and(|b| b.start < first) {
            self.buckets.pop_front();
        }
    }
}

fn bucket_start(time: SystemTime) -> u64 {
    let secs = unix_secs(time);
    secs - secs % ACTIVITY_BUCKET.as_secs()
}

fn first_bucket(last: u64) -> u64 {
    last.saturating_sub((ACTIVITY_BUCKETS as u64 - 1) * ACTIVITY_BUCKET.as_secs())
}

pub fn unix_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

/// How far through the leaf key space `key` lies, from 0 to 1.
pub fn key_fraction(key: &LeafKey) -> f64 {
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&key.bytes[..8]);
    u64::from_be_bytes(prefix) as f64 / u64::MAX as f64
}

impl SyncStatusMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> SyncStatus {
        self.lock().clone()
    }

    pub fn set_phase(&self, shard_group: ShardGroup, phase: GroupPhase) {
        let now = SystemTime::now();
        let mut status = self.lock();
        let group = status.group(shard_group, now);
        if group.phase != phase {
            group.phase = phase;
            group.phase_since = now;
        }
    }

    pub fn set_stream_peer(&self, shard_group: ShardGroup, peer: PeerAddress) {
        self.lock().group(shard_group, SystemTime::now()).stream_peer = Some(peer);
    }

    /// Records a blame of `peers`, and the mismatch it is for when it names the shard whose history diverged.
    pub fn record_blame(
        &self,
        shard_group: ShardGroup,
        epoch: Epoch,
        peers: Vec<PeerAddress>,
        details: &str,
        diverged: Option<Shard>,
    ) {
        let now = SystemTime::now();
        let mut status = self.lock();
        let group = status.group(shard_group, now);
        if group.excluded.0 != epoch {
            group.excluded = (epoch, BTreeSet::new());
        }
        group.excluded.1.extend(peers.iter().copied());
        if group.blamed.len() == MAX_BLAMES {
            group.blamed.pop_front();
        }
        group.blamed.push_back(Blame {
            peers,
            epoch,
            at: now,
            details: details.to_string(),
        });
        if let Some(shard) = diverged {
            status.shards.entry(shard).or_default().last_mismatch = Some((now, details.to_string()));
        }
    }

    pub fn record_tip(
        &self,
        shard_group: ShardGroup,
        epoch: Epoch,
        height: NodeHeight,
        leaves: impl IntoIterator<Item = (Shard, StateVersion)>,
    ) {
        let now = SystemTime::now();
        let mut status = self.lock();
        status.group(shard_group, now).tip = Some(ObservedTip {
            epoch,
            height,
            observed_at: now,
        });
        for (shard, state_version) in leaves {
            status.shards.entry(shard).or_default().tip_leaf = Some((state_version, now));
        }
    }

    pub fn record_checkpoint(&self, shard_group: ShardGroup, epoch: Epoch) {
        let now = SystemTime::now();
        self.lock().group(shard_group, now).last_checkpoint = Some((epoch, now));
    }

    pub fn record_promoted(&self, shard: Shard, versions: u64, transactions: u64) {
        let now = SystemTime::now();
        let mut status = self.lock();
        let shard = status.shards.entry(shard).or_default();
        shard.promoted.record(now, versions, transactions);
        shard.last_promoted_at = Some(now);
    }

    pub fn set_snapshot(&self, shard: Shard, snapshot: Option<SnapshotProgress>) {
        self.lock().shards.entry(shard).or_default().snapshot = snapshot;
    }

    fn lock(&self) -> MutexGuard<'_, SyncStatus> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SyncStatus {
    fn group(&mut self, shard_group: ShardGroup, now: SystemTime) -> &mut GroupStatus {
        self.groups.entry(shard_group).or_insert_with(|| GroupStatus::new(now))
    }
}

#[cfg(test)]
mod tests {
    use tari_crypto::{
        keys::PublicKey,
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    };

    use super::*;

    fn peer(n: u64) -> PeerAddress {
        RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::from(n)).into()
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn activity_in_one_bucket_adds_up() {
        let mut window = ActivityWindow::default();
        window.record(at(1_000), 2, 1);
        window.record(at(1_009), 3, 4);
        let buckets = window.buckets(at(1_009));
        assert_eq!(buckets.len(), ACTIVITY_BUCKETS);
        assert_eq!(*buckets.last().unwrap(), ActivityBucket {
            start: 1_000,
            versions: 5,
            transactions: 5,
        });
        assert!(buckets[..ACTIVITY_BUCKETS - 1].iter().all(|b| b.versions == 0));
    }

    #[test]
    fn activity_buckets_are_dense_and_ascending() {
        let mut window = ActivityWindow::default();
        window.record(at(1_000), 1, 0);
        window.record(at(1_030), 2, 0);
        let buckets = window.buckets(at(1_040));
        let starts = buckets.iter().map(|b| b.start).collect::<Vec<_>>();
        assert!(
            starts
                .windows(2)
                .all(|pair| pair[1] - pair[0] == ACTIVITY_BUCKET.as_secs())
        );
        assert_eq!(*starts.last().unwrap(), 1_040);
        let versions = buckets[ACTIVITY_BUCKETS - 5..]
            .iter()
            .map(|b| b.versions)
            .collect::<Vec<_>>();
        assert_eq!(versions, [1, 0, 0, 2, 0]);
    }

    #[test]
    fn activity_older_than_the_window_is_dropped() {
        let mut window = ActivityWindow::default();
        window.record(at(1_000), 7, 0);
        let later = 1_000 + ACTIVITY_BUCKETS as u64 * ACTIVITY_BUCKET.as_secs();
        window.record(at(later), 1, 0);
        assert_eq!(window.buckets.len(), 1);
        assert!(window.buckets(at(later)).iter().all(|b| b.versions <= 1));
    }

    #[test]
    fn versions_within_counts_only_the_span() {
        let mut window = ActivityWindow::default();
        window.record(at(1_000), 4, 0);
        window.record(at(1_050), 1, 0);
        window.record(at(1_060), 2, 0);
        assert_eq!(window.versions_within(at(1_060), Duration::from_secs(20)), 3);
        assert_eq!(window.versions_within(at(1_060), Duration::from_secs(60)), 3);
        assert_eq!(window.versions_within(at(1_060), Duration::from_secs(70)), 7);
    }

    #[test]
    fn a_phase_keeps_its_start_until_it_changes() {
        let monitor = SyncStatusMonitor::new();
        let group = ShardGroup::new(1, 64);
        monitor.set_phase(group, GroupPhase::Streaming);
        let since = monitor.snapshot().groups[&group].phase_since;
        monitor.set_phase(group, GroupPhase::Streaming);
        assert_eq!(monitor.snapshot().groups[&group].phase_since, since);
        monitor.set_phase(group, GroupPhase::SyncingCheckpoints);
        assert_eq!(monitor.snapshot().groups[&group].phase, GroupPhase::SyncingCheckpoints);
    }

    #[test]
    fn blames_are_capped_and_a_divergence_marks_its_shard() {
        let monitor = SyncStatusMonitor::new();
        let group = ShardGroup::new(1, 64);
        for i in 0..MAX_BLAMES + 2 {
            monitor.record_blame(group, Epoch(1), vec![], &format!("blame {i}"), None);
        }
        monitor.record_blame(group, Epoch(1), vec![], "mismatch", Some(Shard::from(3u32)));
        let status = monitor.snapshot();
        assert_eq!(status.groups[&group].blamed.len(), MAX_BLAMES);
        assert_eq!(status.groups[&group].blamed.back().unwrap().details, "mismatch");
        assert!(status.groups[&group].excluded.1.is_empty());
        assert_eq!(
            status.shards[&Shard::from(3u32)].last_mismatch.as_ref().unwrap().1,
            "mismatch"
        );
    }

    #[test]
    fn every_peer_blamed_in_an_epoch_stays_excluded_past_the_blame_cap() {
        let monitor = SyncStatusMonitor::new();
        let group = ShardGroup::new(1, 64);
        for n in 1..=MAX_BLAMES as u64 + 3 {
            monitor.record_blame(group, Epoch(1), vec![peer(n)], "blame", None);
        }
        let excluded = monitor.snapshot().groups[&group].excluded.clone();
        assert_eq!(excluded.0, Epoch(1));
        assert_eq!(excluded.1.len(), MAX_BLAMES + 3);

        monitor.record_blame(group, Epoch(2), vec![peer(1)], "blame", None);
        let excluded = monitor.snapshot().groups[&group].excluded.clone();
        assert_eq!(excluded, (Epoch(2), BTreeSet::from([peer(1)])));
    }

    #[test]
    fn a_key_fraction_spans_the_key_space() {
        let low = LeafKey::new([0u8; 32].into());
        let high = LeafKey::new([0xffu8; 32].into());
        let mut mid = [0u8; 32];
        mid[0] = 0x80;
        assert_eq!(key_fraction(&low), 0.0);
        assert_eq!(key_fraction(&high), 1.0);
        assert!((key_fraction(&LeafKey::new(mid.into())) - 0.5).abs() < 1e-9);
    }
}
