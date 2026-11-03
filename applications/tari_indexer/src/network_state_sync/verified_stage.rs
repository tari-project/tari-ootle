//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Recomputes each streamed shard's transition accumulator from the versions it is streamed, and checks it against
//! quorum-verified shard leaves.
//!
//! The accumulator is cumulative, so a leaf that matches the accumulator computed at its version authenticates every
//! version streamed up to it.

use std::collections::{BTreeMap, HashMap, VecDeque};

use ootle_network::Network;
use tari_engine_types::{published_template::PublishedTemplateAddress, substate::hash_published_template_metadata};
use tari_ootle_common_types::{Epoch, StateVersion, shard::Shard};
use tari_ootle_storage::consensus_models::{EpochCheckpoint, SubstateUpdateProof};
use tari_state_tree::{StateTreeError, SubstateTreeChange, TransitionAccumulator};
use tari_template_lib_types::Hash32;

/// The most staged versions a shard keeps the accumulator of in memory, newest kept, for a leaf that trails the
/// stream. A tip leaf comes from a committee member probed in the last few seconds, so it trails the stream by a
/// handful of versions; a leaf further behind checks nothing a later one does not.
pub const MAX_ACCUMULATOR_HISTORY: usize = 256;

/// The most leaves a shard holds ahead of the stream. A catch-up meets a checkpoint leaf per epoch on its way to the
/// tip, and each one it holds promotes what was staged below it; past this many, those between the lowest and the
/// highest are dropped first. Checkpoint leaves are stored as well, and the next few above a shard's verified version
/// are checked again after each promotion, so a dropped one still checks the versions below it.
pub const MAX_WAITING_LEAVES: usize = 64;

/// A shard's state version and transition accumulator as a quorum committed them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedShardLeaf {
    pub shard: Shard,
    pub state_version: StateVersion,
    pub transition_acc: TransitionAccumulator,
}

impl VerifiedShardLeaf {
    /// The leaves of every shard `checkpoint` summarises but the global shard. Every shard group commits its own copy
    /// of the global shard, and the stream follows only one of them.
    pub fn from_checkpoint(checkpoint: &EpochCheckpoint) -> impl Iterator<Item = Self> + '_ {
        checkpoint
            .shard_tree_summary()
            .iter()
            .filter(|(shard, _)| !shard.is_global())
            .map(|(shard, summary)| Self {
                shard: *shard,
                state_version: StateVersion::new(summary.state_version),
                transition_acc: summary.transition_acc,
            })
    }
}

/// The outcome of checking a shard's computed accumulator against a verified leaf. `P` names who streamed a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict<P> {
    /// The accumulator computed at the version equals the leaf's, which authenticates every version staged up to it.
    Promote(StateVersion),
    /// The accumulator computed at `version` differs from the leaf's. The shard is reset to its last verified
    /// version, and every version staged above it is to be discarded.
    Mismatch {
        version: StateVersion,
        epoch: Epoch,
        computed: TransitionAccumulator,
        verified: TransitionAccumulator,
        streamed_by: P,
    },
    /// The leaf disagrees with the accumulator the shard's last verified version holds. A quorum signed both, so the
    /// fault lies with the signers or with this indexer's leaf verification.
    Conflict {
        version: StateVersion,
        held: TransitionAccumulator,
        verified: TransitionAccumulator,
    },
    /// The stream has not reached the leaf's version. The leaf is checked once it does.
    Wait,
    /// The leaf checks nothing new: its version is at or below the last verified one, or below every accumulator the
    /// shard holds.
    Stale,
}

/// A shard's state version and the transition accumulator after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccumulatedVersion {
    pub version: StateVersion,
    pub acc: TransitionAccumulator,
}

/// A staged version's accumulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged<P> {
    pub version: StateVersion,
    pub epoch: Epoch,
    pub acc: TransitionAccumulator,
    pub streamed_by: P,
}

/// The transition accumulator of every shard the indexer streams, from its last verified version through the versions
/// staged above it, checked against verified leaves.
#[derive(Debug)]
pub struct VerifiedStage<P> {
    shards: HashMap<Shard, ShardStage<P>>,
}

impl<P> Default for VerifiedStage<P> {
    fn default() -> Self {
        Self { shards: HashMap::new() }
    }
}

impl<P: Clone> VerifiedStage<P> {
    /// Resets `shard` to its last verified version, `verified`, and the versions staged above it, whose last
    /// [`MAX_ACCUMULATOR_HISTORY`] are `staged`, ascending. A leaf the shard was waiting for is kept, and checked if
    /// the staged versions reach it. A shard with nothing verified has `verified` of `None`.
    pub fn restore(
        &mut self,
        shard: Shard,
        verified: Option<AccumulatedVersion>,
        staged: Vec<Staged<P>>,
    ) -> Result<Option<Verdict<P>>, StageError> {
        // The tail starts anywhere above the verified version once more than it holds are staged, and runs
        // contiguously from there.
        let mut previous = verified.map(|verified| verified.version);
        for (i, staged) in staged.iter().enumerate() {
            let in_order = if i == 0 {
                previous.is_none_or(|previous| staged.version > previous)
            } else {
                follows(previous, staged.version)
            };
            if !in_order {
                return Err(StageError::NotNext {
                    head: previous,
                    version: staged.version,
                });
            }
            previous = Some(staged.version);
        }
        let stage = self.shards.entry(shard).or_default();
        stage.verified = verified;
        stage.head = staged.last().map_or(verified, |last| {
            Some(AccumulatedVersion {
                version: last.version,
                acc: last.acc,
            })
        });
        stage.history = staged.into_iter().collect();
        while stage.history.len() > MAX_ACCUMULATOR_HISTORY {
            stage.history.pop_front();
        }
        Ok(stage.check_waiting())
    }

    /// Extends `shard`'s accumulator with `version`, committed at `epoch` with `changes` and streamed by
    /// `streamed_by`, and checks a leaf that was waiting for it. Returns the accumulator after the version.
    ///
    /// A version that does not follow the shard's head, or whose transitions no committed version could hold, is
    /// refused and leaves the stage as it was.
    pub fn on_version(
        &mut self,
        shard: Shard,
        version: StateVersion,
        epoch: Epoch,
        changes: &[SubstateTreeChange],
        streamed_by: P,
    ) -> Result<(TransitionAccumulator, Option<Verdict<P>>), StageError> {
        let stage = self.shards.entry(shard).or_default();
        let head = stage.head;
        let head_version = head.map(|head| head.version);
        if !follows(head_version, version) {
            return Err(StageError::NotNext {
                head: head_version,
                version,
            });
        }
        let acc = head
            .map_or(TransitionAccumulator::zero(), |head| head.acc)
            .extend(epoch, version.as_u64(), changes)
            .map_err(|source| StageError::Uncomputable { version, source })?;
        stage.head = Some(AccumulatedVersion { version, acc });
        stage.history.push_back(Staged {
            version,
            epoch,
            acc,
            streamed_by,
        });
        if stage.history.len() > MAX_ACCUMULATOR_HISTORY {
            stage.history.pop_front();
        }
        Ok((acc, stage.check_waiting()))
    }

    pub fn on_leaf(&mut self, leaf: VerifiedShardLeaf) -> Verdict<P> {
        self.shards.entry(leaf.shard).or_default().check(leaf)
    }

    /// Whether a leaf of `shard` would check anything: versions are staged above its last verified one, and no leaf
    /// is already waiting for the stream.
    pub fn wants_leaf(&self, shard: Shard) -> bool {
        self.shards
            .get(&shard)
            .is_some_and(|stage| stage.waiting.is_empty() && version_of(stage.head) > version_of(stage.verified))
    }

    /// `shard`'s last verified version, or `None` when nothing of it is verified.
    pub fn verified_version(&self, shard: Shard) -> Option<StateVersion> {
        version_of(self.shards.get(&shard)?.verified)
    }

    /// The version of the lowest verified leaf `shard` is waiting for the stream to reach.
    pub fn waiting_for(&self, shard: Shard) -> Option<StateVersion> {
        self.shards.get(&shard)?.waiting.keys().next().copied()
    }

    /// The highest version any verified leaf of `shard` has named.
    pub fn highest_leaf(&self, shard: Shard) -> Option<StateVersion> {
        self.shards.get(&shard)?.highest_leaf
    }
}

/// Whether `version` is the one after `head`, the last version staged or verified. A shard's history starts at v0, so
/// v0 follows a shard with nothing staged or verified.
pub fn follows(head: Option<StateVersion>, version: StateVersion) -> bool {
    match head {
        Some(head) => head.as_u64().checked_add(1) == Some(version.as_u64()),
        None => version.as_u64() == 0,
    }
}

fn version_of(position: Option<AccumulatedVersion>) -> Option<StateVersion> {
    position.map(|position| position.version)
}

#[derive(Debug)]
struct ShardStage<P> {
    /// The last version a verified leaf authenticated, and its accumulator, or `None` while no version is.
    verified: Option<AccumulatedVersion>,
    /// The last version staged, or the verified one when none is, and its accumulator, or `None` while neither is.
    head: Option<AccumulatedVersion>,
    /// The accumulator after each version staged above the verified one, oldest first.
    history: VecDeque<Staged<P>>,
    /// The leaves ahead of the stream, by version.
    waiting: BTreeMap<StateVersion, VerifiedShardLeaf>,
    highest_leaf: Option<StateVersion>,
}

impl<P> Default for ShardStage<P> {
    fn default() -> Self {
        Self {
            verified: None,
            head: None,
            history: VecDeque::new(),
            waiting: BTreeMap::new(),
            highest_leaf: None,
        }
    }
}

impl<P: Clone> ShardStage<P> {
    fn check(&mut self, leaf: VerifiedShardLeaf) -> Verdict<P> {
        self.highest_leaf = self.highest_leaf.max(Some(leaf.state_version));
        if let Some(AccumulatedVersion {
            version: verified_version,
            acc: held,
        }) = self.verified
        {
            if leaf.state_version == verified_version {
                return if held == leaf.transition_acc {
                    Verdict::Stale
                } else {
                    Verdict::Conflict {
                        version: verified_version,
                        held,
                        verified: leaf.transition_acc,
                    }
                };
            }
            if leaf.state_version < verified_version {
                return Verdict::Stale;
            }
        }
        if version_of(self.head).is_none_or(|head| leaf.state_version > head) {
            self.waiting.entry(leaf.state_version).or_insert(leaf);
            if self.waiting.len() > MAX_WAITING_LEAVES {
                let below_highest = self.waiting.keys().rev().nth(1).copied();
                if let Some(version) = below_highest {
                    self.waiting.remove(&version);
                }
            }
            return Verdict::Wait;
        }
        let Some(position) = self
            .history
            .iter()
            .position(|staged| staged.version == leaf.state_version)
        else {
            return Verdict::Stale;
        };
        let staged = &self.history[position];
        if staged.acc != leaf.transition_acc {
            let verdict = Verdict::Mismatch {
                version: staged.version,
                epoch: staged.epoch,
                computed: staged.acc,
                verified: leaf.transition_acc,
                streamed_by: staged.streamed_by.clone(),
            };
            self.head = self.verified;
            self.history.clear();
            return verdict;
        }
        self.history.drain(..=position);
        self.verified = Some(AccumulatedVersion {
            version: leaf.state_version,
            acc: leaf.transition_acc,
        });
        Verdict::Promote(leaf.state_version)
    }

    /// Checks the leaves the stream has reached, lowest first, returning the verdict of the last one, or of the
    /// first that mismatches. A mismatch rewinds the head, so the leaves above it wait again.
    fn check_waiting(&mut self) -> Option<Verdict<P>> {
        let mut verdict = None;
        while let Some(entry) = self.waiting.first_entry() &&
            version_of(self.head).is_some_and(|head| *entry.key() <= head)
        {
            let leaf = entry.remove();
            match self.check(leaf) {
                mismatch @ Verdict::Mismatch { .. } => return Some(mismatch),
                Verdict::Stale => {},
                checked => verdict = Some(checked),
            }
        }
        verdict
    }
}

/// The tree change a streamed transition makes.
///
/// A template is streamed as its metadata and value hash. The hash is a function of the metadata, so it is derived
/// from the metadata, and a sent hash or template address that disagrees with the metadata is rejected.
pub fn streamed_tree_change(
    network: Network,
    epoch: Epoch,
    update: &SubstateUpdateProof,
) -> Result<SubstateTreeChange, StreamedTransitionError> {
    let Some(create) = update.as_create() else {
        return Ok(update.to_tree_change(network, epoch));
    };
    let (Some(metadata), Some(address)) = (
        &create.substate.template_metadata,
        create.substate.substate_id().as_template(),
    ) else {
        return Ok(update.to_tree_change(network, epoch));
    };
    if PublishedTemplateAddress::from_author_and_binary_hash(&metadata.author_public_key, &metadata.binary_hash) !=
        address
    {
        return Err(StreamedTransitionError::TemplateAddress { address });
    }
    let value_hash = hash_published_template_metadata(network, metadata, create.substate.version, epoch);
    let sent = create.substate.to_value_hash(network, epoch);
    if sent != value_hash {
        return Err(StreamedTransitionError::TemplateValueHash {
            address,
            sent,
            derived: value_hash,
        });
    }
    Ok(SubstateTreeChange::Up {
        id: create.substate.as_versioned_substate_id_ref().to_owned(),
        value_hash,
    })
}

/// A streamed version the stage cannot extend a shard's accumulator with.
#[derive(Debug, thiserror::Error)]
pub enum StageError {
    #[error("v{version} does not follow {}", head.map_or("the start of the shard's history".to_string(), |head| format!("v{head}, the last version staged")))]
    NotNext {
        head: Option<StateVersion>,
        version: StateVersion,
    },
    #[error("no committed version could hold the transitions of v{version}: {source}")]
    Uncomputable {
        version: StateVersion,
        source: StateTreeError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum StreamedTransitionError {
    #[error("template {address} was streamed with metadata whose author and binary hash derive another address")]
    TemplateAddress { address: PublishedTemplateAddress },
    #[error("template {address} was streamed with value hash {sent}, but its metadata hashes to {derived}")]
    TemplateValueHash {
        address: PublishedTemplateAddress,
        sent: Hash32,
        derived: Hash32,
    },
}

#[cfg(test)]
mod tests {
    use tari_engine_types::{SubstateVersion, published_template::PublishedTemplateMetadata, substate::SubstateId};
    use tari_ootle_common_types::VersionedSubstateId;
    use tari_ootle_storage::consensus_models::{SubstateCreate, SubstateData, SubstateDestroy, SubstateValueOrHash};
    use tari_template_lib_types::{ComponentAddress, ObjectKey, crypto::RistrettoPublicKeyBytes};

    use super::*;

    const EPOCH: Epoch = Epoch(3);
    const PEER: u8 = 9;

    fn shard() -> Shard {
        Shard::from(3u32)
    }

    fn v(n: u64) -> StateVersion {
        StateVersion::new(n)
    }

    fn id(seed: u64) -> VersionedSubstateId {
        let mut key = [0; ObjectKey::LENGTH];
        key[..8].copy_from_slice(&seed.to_le_bytes());
        VersionedSubstateId::new(
            SubstateId::Component(ComponentAddress::new(ObjectKey::from_array(key))),
            SubstateVersion::ZERO,
        )
    }

    fn up(seed: u64, value: u8) -> SubstateTreeChange {
        SubstateTreeChange::Up {
            id: id(seed),
            value_hash: Hash32::from_array([value; 32]),
        }
    }

    fn version_changes(version: u64) -> Vec<SubstateTreeChange> {
        vec![up(version, 1), up(version + 1000, 2)]
    }

    /// A shard's committed history: the transitions of each version from v0, and the accumulator after each.
    struct History {
        versions: Vec<Vec<SubstateTreeChange>>,
        accs: Vec<TransitionAccumulator>,
    }

    impl History {
        fn new(num_versions: u64) -> Self {
            let versions = (0..=num_versions).map(version_changes).collect::<Vec<_>>();
            let mut acc = TransitionAccumulator::zero();
            let accs = versions
                .iter()
                .zip(0..)
                .map(|(changes, n)| {
                    acc = acc.extend(EPOCH, n, changes).unwrap();
                    acc
                })
                .collect();
            Self { versions, accs }
        }

        fn changes(&self, version: u64) -> &[SubstateTreeChange] {
            &self.versions[usize::try_from(version).unwrap()]
        }

        fn leaf(&self, version: u64) -> VerifiedShardLeaf {
            VerifiedShardLeaf {
                shard: shard(),
                state_version: v(version),
                transition_acc: self.accs[usize::try_from(version).unwrap()],
            }
        }

        fn stream(&self, stage: &mut VerifiedStage<u8>, versions: impl IntoIterator<Item = u64>) -> Vec<Verdict<u8>> {
            versions
                .into_iter()
                .filter_map(|n| {
                    let (_, verdict) = stage.on_version(shard(), v(n), EPOCH, self.changes(n), PEER).unwrap();
                    verdict
                })
                .collect()
        }
    }

    #[test]
    fn a_leaf_at_the_head_promotes_every_version_staged() {
        let history = History::new(3);
        let mut stage = VerifiedStage::default();
        assert!(history.stream(&mut stage, 0..=3).is_empty());
        assert!(stage.wants_leaf(shard()));

        assert_eq!(stage.on_leaf(history.leaf(3)), Verdict::Promote(v(3)));
        assert!(!stage.wants_leaf(shard()));
        assert_eq!(stage.on_leaf(history.leaf(3)), Verdict::Stale);
    }

    #[test]
    fn a_leaf_behind_the_stream_promotes_up_to_its_version() {
        let history = History::new(5);
        let mut stage = VerifiedStage::default();
        history.stream(&mut stage, 0..=5);

        assert_eq!(stage.on_leaf(history.leaf(3)), Verdict::Promote(v(3)));
        assert!(stage.wants_leaf(shard()));
        assert_eq!(stage.on_leaf(history.leaf(2)), Verdict::Stale);
        assert_eq!(stage.on_leaf(history.leaf(5)), Verdict::Promote(v(5)));
    }

    #[test]
    fn a_leaf_ahead_of_the_stream_is_checked_when_the_stream_reaches_it() {
        let history = History::new(4);
        let mut stage = VerifiedStage::default();
        history.stream(&mut stage, 0..=1);

        assert_eq!(stage.on_leaf(history.leaf(4)), Verdict::Wait);
        assert_eq!(stage.on_leaf(history.leaf(3)), Verdict::Wait);
        assert!(!stage.wants_leaf(shard()));
        assert_eq!(stage.waiting_for(shard()), Some(v(3)));
        assert_eq!(stage.highest_leaf(shard()), Some(v(4)));

        assert_eq!(history.stream(&mut stage, 2..=3), vec![Verdict::Promote(v(3))]);
        assert_eq!(stage.waiting_for(shard()), Some(v(4)));
        assert_eq!(history.stream(&mut stage, 4..=4), vec![Verdict::Promote(v(4))]);
        assert_eq!(stage.waiting_for(shard()), None);
    }

    #[test]
    fn a_catch_up_promotes_at_each_leaf_it_reaches() {
        let history = History::new(9);
        let mut stage = VerifiedStage::default();
        for version in [3, 9, 6] {
            assert_eq!(stage.on_leaf(history.leaf(version)), Verdict::Wait);
        }

        let verdicts = history.stream(&mut stage, 0..=9);
        assert_eq!(verdicts, [3, 6, 9].map(|n| Verdict::Promote(v(n))));
    }

    #[test]
    fn a_catch_up_mismatches_at_the_first_leaf_past_a_tampered_version() {
        let history = History::new(6);
        let mut stage = VerifiedStage::default();
        stage.on_leaf(history.leaf(2));
        stage.on_leaf(history.leaf(5));
        history.stream(&mut stage, 0..=3);
        stage.on_version(shard(), v(4), EPOCH, &[up(77, 1)], PEER).unwrap();

        let (_, verdict) = stage
            .on_version(shard(), v(5), EPOCH, history.changes(5), PEER)
            .unwrap();
        assert!(matches!(verdict, Some(Verdict::Mismatch { version, .. }) if version == v(5)));
        assert_eq!(stage.verified_version(shard()), Some(v(2)));
        assert_eq!(stage.on_leaf(history.leaf(5)), Verdict::Wait);
        assert_eq!(history.stream(&mut stage, 3..=5), vec![Verdict::Promote(v(5))]);
    }

    #[test]
    fn past_the_most_waiting_leaves_the_lowest_and_highest_are_kept() {
        let num_versions = MAX_WAITING_LEAVES as u64 + 5;
        let history = History::new(num_versions);
        let mut stage = VerifiedStage::default();
        for version in 1..=num_versions {
            stage.on_leaf(history.leaf(version));
        }

        assert_eq!(stage.waiting_for(shard()), Some(v(1)));
        let verdicts = history.stream(&mut stage, 0..=num_versions);
        assert_eq!(verdicts.len(), MAX_WAITING_LEAVES);
        assert_eq!(verdicts.last(), Some(&Verdict::Promote(v(num_versions))));
    }

    type Tamper = dyn Fn(&mut Vec<SubstateTreeChange>, &mut Epoch);

    #[test]
    fn a_history_that_differs_from_the_committed_one_mismatches_and_rewinds_to_the_verified_version() {
        let history = History::new(3);
        let tampered: [(&str, Box<Tamper>); 4] = [
            ("a tampered value", Box::new(|changes, _| changes[0] = up(2, 7))),
            ("an omitted transition", Box::new(|changes, _| drop(changes.pop()))),
            ("an extra transition", Box::new(|changes, _| changes.push(up(77, 1)))),
            ("a wrong epoch", Box::new(|_, epoch| *epoch = Epoch(4))),
        ];
        for (case, tamper) in tampered {
            let mut stage = VerifiedStage::default();
            history.stream(&mut stage, 0..=1);
            assert_eq!(stage.on_leaf(history.leaf(1)), Verdict::Promote(v(1)), "{case}");
            let mut changes = version_changes(2);
            let mut epoch = EPOCH;
            tamper(&mut changes, &mut epoch);
            stage.on_version(shard(), v(2), epoch, &changes, PEER).unwrap();
            history.stream(&mut stage, 3..=3);

            let Verdict::Mismatch {
                version,
                epoch,
                computed,
                verified,
                streamed_by,
            } = stage.on_leaf(history.leaf(3))
            else {
                panic!("{case} is not a mismatch");
            };
            assert_eq!(
                (version, epoch, verified, streamed_by),
                (v(3), EPOCH, history.accs[3], PEER),
                "{case}"
            );
            assert_ne!(computed, verified, "{case}");
            assert!(!stage.wants_leaf(shard()), "{case} leaves nothing staged");

            assert!(history.stream(&mut stage, 2..=3).is_empty(), "{case}");
            assert_eq!(stage.on_leaf(history.leaf(3)), Verdict::Promote(v(3)), "{case}");
        }
    }

    #[test]
    fn a_version_that_does_not_follow_the_head_is_refused() {
        let history = History::new(4);
        let mut stage = VerifiedStage::default();
        history.stream(&mut stage, 0..=2);

        for version in [1, 2, 4] {
            let result = stage.on_version(shard(), v(version), EPOCH, history.changes(version), PEER);
            assert!(
                matches!(result, Err(StageError::NotNext { head, .. }) if head == Some(v(2))),
                "v{version}"
            );
        }
        history.stream(&mut stage, 3..=4);
        assert_eq!(stage.on_leaf(history.leaf(4)), Verdict::Promote(v(4)));
    }

    #[test]
    fn a_version_no_block_could_commit_is_refused_and_leaves_the_stage_as_it_was() {
        let history = History::new(2);
        let mut stage = VerifiedStage::default();
        history.stream(&mut stage, 0..=1);

        let result = stage.on_version(shard(), v(2), EPOCH, &[up(1, 1), up(1, 2)], PEER);
        assert!(matches!(result, Err(StageError::Uncomputable { version, .. }) if version == v(2)));
        history.stream(&mut stage, 2..=2);
        assert_eq!(stage.on_leaf(history.leaf(2)), Verdict::Promote(v(2)));
    }

    #[test]
    fn a_leaf_that_disagrees_with_the_verified_version_conflicts() {
        let history = History::new(3);
        let forged = |version| VerifiedShardLeaf {
            transition_acc: TransitionAccumulator::new([7; 32]),
            ..history.leaf(version)
        };

        let mut stage = VerifiedStage::default();
        history.stream(&mut stage, 0..=3);
        assert_eq!(stage.on_leaf(history.leaf(2)), Verdict::Promote(v(2)));
        assert_eq!(stage.on_leaf(forged(2)), Verdict::Conflict {
            version: v(2),
            held: history.accs[2],
            verified: forged(2).transition_acc,
        });
        assert_eq!(stage.on_leaf(history.leaf(3)), Verdict::Promote(v(3)));
    }

    #[test]
    fn a_shard_nothing_was_staged_for_wants_no_leaf_and_holds_one_until_the_stream_reaches_it() {
        let history = History::new(1);
        let mut stage = VerifiedStage::<u8>::default();
        assert!(!stage.wants_leaf(shard()));
        assert_eq!(stage.verified_version(shard()), None);

        assert_eq!(stage.on_leaf(history.leaf(0)), Verdict::Wait);
        assert_eq!(history.stream(&mut stage, 0..=0), vec![Verdict::Promote(v(0))]);
        assert_eq!(stage.on_leaf(history.leaf(1)), Verdict::Wait);
        assert_eq!(history.stream(&mut stage, 1..=1), vec![Verdict::Promote(v(1))]);
    }

    #[test]
    fn a_shard_history_starts_at_v0_and_an_empty_v0_leaves_the_accumulator_at_zero() {
        let mut stage = VerifiedStage::default();
        let result = stage.on_version(shard(), v(1), EPOCH, &version_changes(1), PEER);
        assert!(matches!(result, Err(StageError::NotNext { head: None, .. })));

        let (acc, verdict) = stage.on_version(shard(), v(0), Epoch(0), &[], PEER).unwrap();
        assert_eq!((acc, verdict), (TransitionAccumulator::zero(), None));
        let genesis = VerifiedShardLeaf {
            shard: shard(),
            state_version: v(0),
            transition_acc: TransitionAccumulator::zero(),
        };
        assert_eq!(stage.on_leaf(genesis), Verdict::Promote(v(0)));
        assert_eq!(stage.verified_version(shard()), Some(v(0)));

        let changes = version_changes(1);
        let (acc, _) = stage.on_version(shard(), v(1), EPOCH, &changes, PEER).unwrap();
        assert_eq!(acc, TransitionAccumulator::zero().extend(EPOCH, 1, &changes).unwrap());
    }

    fn staged(history: &History, versions: impl IntoIterator<Item = u64>) -> Vec<Staged<u8>> {
        versions
            .into_iter()
            .map(|n| Staged {
                version: v(n),
                epoch: EPOCH,
                acc: history.leaf(n).transition_acc,
                streamed_by: PEER,
            })
            .collect()
    }

    #[test]
    fn a_restored_shard_continues_from_its_staged_versions() {
        let history = History::new(5);
        let mut stage = VerifiedStage::default();
        let verified = Some(AccumulatedVersion {
            version: v(1),
            acc: history.accs[1],
        });

        assert_eq!(stage.restore(shard(), verified, staged(&history, 2..=3)).unwrap(), None);
        assert!(stage.wants_leaf(shard()));
        history.stream(&mut stage, 4..=5);
        assert_eq!(stage.on_leaf(history.leaf(3)), Verdict::Promote(v(3)));
        assert_eq!(stage.on_leaf(history.leaf(5)), Verdict::Promote(v(5)));

        // A restore discards what the stage held above the verified version.
        let mut stage = VerifiedStage::default();
        history.stream(&mut stage, 0..=5);
        stage.restore(shard(), verified, Vec::new()).unwrap();
        assert!(!stage.wants_leaf(shard()));
        assert_eq!(history.stream(&mut stage, 2..=2), vec![]);
    }

    #[test]
    fn a_restored_shard_checks_the_leaf_it_was_waiting_for() {
        let history = History::new(3);
        let mut stage = VerifiedStage::default();
        assert_eq!(stage.on_leaf(history.leaf(3)), Verdict::Wait);

        let verdict = stage.restore(shard(), None, staged(&history, 0..=3)).unwrap();
        assert_eq!(verdict, Some(Verdict::Promote(v(3))));
    }

    #[test]
    fn a_restored_tail_that_is_not_contiguous_above_the_verified_version_is_refused() {
        let history = History::new(6);
        let mut stage = VerifiedStage::default();
        let verified = Some(AccumulatedVersion {
            version: v(2),
            acc: history.accs[2],
        });

        stage.restore(shard(), verified, staged(&history, 4..=6)).unwrap();
        assert!(stage.restore(shard(), verified, staged(&history, 2..=3)).is_err());
        assert!(stage.restore(shard(), verified, staged(&history, [3, 5])).is_err());
    }

    fn template_metadata() -> PublishedTemplateMetadata {
        PublishedTemplateMetadata {
            template_name: "Counter".to_string(),
            author_public_key: RistrettoPublicKeyBytes::default(),
            binary_hash: Hash32::from_array([5; 32]),
            at_epoch: 1,
            metadata_hash: None,
        }
    }

    fn streamed_template(
        address: PublishedTemplateAddress,
        metadata: PublishedTemplateMetadata,
        value_hash: Hash32,
    ) -> SubstateUpdateProof {
        SubstateCreate {
            substate: SubstateData {
                substate_id: SubstateId::Template(address),
                version: SubstateVersion::ZERO,
                value: SubstateValueOrHash::Hash(value_hash),
                template_metadata: Some(metadata),
            },
        }
        .into()
    }

    fn address_of(metadata: &PublishedTemplateMetadata) -> PublishedTemplateAddress {
        PublishedTemplateAddress::from_author_and_binary_hash(&metadata.author_public_key, &metadata.binary_hash)
    }

    #[test]
    fn a_template_is_hashed_from_its_metadata() {
        let metadata = template_metadata();
        let value_hash = hash_published_template_metadata(Network::LocalNet, &metadata, SubstateVersion::ZERO, EPOCH);
        let update = streamed_template(address_of(&metadata), metadata, value_hash);

        let change = streamed_tree_change(Network::LocalNet, EPOCH, &update).unwrap();
        assert!(
            matches!(&change, SubstateTreeChange::Up { id, value_hash: hash } if *id == update.to_versioned_substate_id() && *hash == value_hash)
        );
    }

    #[test]
    fn a_template_whose_metadata_disagrees_with_its_hash_or_address_is_rejected() {
        let metadata = template_metadata();
        let value_hash = hash_published_template_metadata(Network::LocalNet, &metadata, SubstateVersion::ZERO, EPOCH);
        let renamed = PublishedTemplateMetadata {
            template_name: "Renamed".to_string(),
            ..metadata.clone()
        };
        let update = streamed_template(address_of(&metadata), renamed, value_hash);
        assert!(matches!(
            streamed_tree_change(Network::LocalNet, EPOCH, &update),
            Err(StreamedTransitionError::TemplateValueHash { .. })
        ));

        let other_binary = PublishedTemplateMetadata {
            binary_hash: Hash32::from_array([6; 32]),
            ..metadata.clone()
        };
        let update = streamed_template(address_of(&metadata), other_binary, value_hash);
        assert!(matches!(
            streamed_tree_change(Network::LocalNet, EPOCH, &update),
            Err(StreamedTransitionError::TemplateAddress { .. })
        ));
    }

    #[test]
    fn a_destroyed_substate_is_a_down() {
        let id = id(1);
        let update = SubstateUpdateProof::Destroy(SubstateDestroy {
            substate_id: id.substate_id().clone(),
            version: id.version(),
        });
        let change = streamed_tree_change(Network::LocalNet, EPOCH, &update).unwrap();
        assert!(matches!(change, SubstateTreeChange::Down { id: down } if down == id));
    }
}
