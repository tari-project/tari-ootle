//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use tari_ootle_common_types::{Epoch, NodeHeight, committee::Committee};
use tari_ootle_storage::{
    StateStoreReadTransaction,
    StorageError,
    consensus_models::{LivenessThresholds, ValidatorConsensusStats},
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::traits::LeaderStrategy;

/// How far below a block's justify the liveness state that decides its proposer is read.
///
/// A block's justify certifies the block one view below it, and committing that block is what a
/// 3-chain ending there does, three views further back. Every node that acts on a view has therefore
/// committed `justify_height - LIVENESS_ANCHOR_LAG` and knows every liveness log entry up to it,
/// while a node that has run further ahead still answers the same query from the log. Reading the
/// latest state instead would let two nodes at different heights pick different leaders.
const LIVENESS_ANCHOR_LAG: NodeHeight = NodeHeight(3);

/// The validators whose slot in the leader rotation is skipped for views anchored on one committed
/// height.
///
/// One anchor decides the proposer of a block, the attribution of every dummy block below it and the
/// leader that collects the votes for it, so a set is loaded per anchor and then answers for all of
/// them. Loading it costs one lookup per committee member, so a caller that answers the same
/// question for many messages of one view - the leader collecting its votes - keeps the set it
/// loaded rather than rebuilding it per message.
#[derive(Debug, Clone, Default)]
pub struct LeaderSkipSet {
    skipped: Vec<RistrettoPublicKeyBytes>,
}

impl LeaderSkipSet {
    /// A set that skips nobody: every view goes to the leader strategy's leader.
    pub fn none() -> Self {
        Self { skipped: Vec::new() }
    }

    /// Loads the set that decides the leaders of the views a block justified at `justify_height`
    /// covers.
    pub fn load_for_justify<TTx: StateStoreReadTransaction, TAddr: PartialEq>(
        tx: &TTx,
        epoch: Epoch,
        justify_height: NodeHeight,
        committee: &Committee<TAddr>,
        thresholds: &LivenessThresholds,
    ) -> Result<Self, StorageError> {
        let anchor = justify_height.saturating_sub(LIVENESS_ANCHOR_LAG);
        let mut skipped = Vec::new();
        for public_key in committee.public_keys() {
            let counters = ValidatorConsensusStats::liveness_counters_as_of(tx, epoch, public_key, anchor)?;
            if counters.is_skipped(thresholds, anchor) {
                skipped.push(*public_key);
            }
        }

        Ok(Self { skipped })
    }

    pub fn is_empty(&self) -> bool {
        self.skipped.is_empty()
    }

    pub fn is_skipped(&self, public_key: &RistrettoPublicKeyBytes) -> bool {
        self.skipped.contains(public_key)
    }

    /// The validator that proposes for `view_height`: the first of `leader(view_height)`,
    /// `leader(view_height + 1)`, … that is not skipped.
    ///
    /// Views are remapped rather than skipped: the effective leader collects the votes for the block
    /// at `view_height` and proposes the next one, so a skipped validator costs no dummy block, no
    /// timeout certificate and no NEWVIEW.
    ///
    /// The walk is bounded by the committee's total power, which covers every member with power
    /// because [`LeaderStrategy::calculate_leader`] gives each member `vote_power` of any
    /// `total_power` consecutive heights - a strategy that does not would need a different bound.
    /// It falls back to the strategy's leader when every candidate it visits is skipped, which
    /// cannot happen while less than a third of the committee's power is faulty but keeps the
    /// function total.
    ///
    /// `view_height` may come from a peer message, so the walk wraps past the top of the height
    /// range to height zero. Its slots are then two consecutive runs, one either side of the wrap,
    /// and the longer covers at least half of the schedule's slots. Skipped members hold fewer than
    /// a third of them, so the fallback stays unreachable while less than a third of the committee's
    /// power is faulty.
    pub fn effective_leader<'a, TAddr: PartialEq, TLeaderStrategy: LeaderStrategy<TAddr>>(
        &self,
        leader_strategy: &TLeaderStrategy,
        committee: &'a Committee<TAddr>,
        view_height: NodeHeight,
    ) -> (&'a TAddr, &'a RistrettoPublicKeyBytes) {
        let raw = leader_strategy.get_leader(committee, view_height);
        if self.skipped.is_empty() {
            return raw;
        }

        let period = usize::try_from(committee.total_power().value()).unwrap_or(usize::MAX);
        for index in leader_strategy.leaders_from(committee, view_height).take(period) {
            let member = committee
                .get(index as usize)
                .expect("the leader strategy picks an index into the committee");
            if !self.is_skipped(&member.public_key) {
                return (&member.address, &member.public_key);
            }
        }

        raw
    }

    pub fn is_effective_leader<TAddr: PartialEq, TLeaderStrategy: LeaderStrategy<TAddr>>(
        &self,
        leader_strategy: &TLeaderStrategy,
        committee: &Committee<TAddr>,
        view_height: NodeHeight,
        addr: &TAddr,
    ) -> bool {
        self.effective_leader(leader_strategy, committee, view_height).0 == addr
    }
}

impl Display for LeaderSkipSet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.skipped.is_empty() {
            return write!(f, "no skipped leaders");
        }
        write!(f, "skipped leaders: ")?;
        for (i, pk) in self.skipped.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{pk}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::committee::CommitteeMember;

    use super::*;
    use crate::leader_selection::SmoothWeightedRoundRobin;

    fn pk(n: u8) -> RistrettoPublicKeyBytes {
        let mut bytes = [0u8; 32];
        bytes[0] = n;
        RistrettoPublicKeyBytes::from_bytes(&bytes).unwrap()
    }

    fn committee(n: u8) -> Committee<u8> {
        weighted_committee(&vec![1; n as usize])
    }

    fn weighted_committee(powers: &[u64]) -> Committee<u8> {
        Committee::new(
            powers
                .iter()
                .enumerate()
                .map(|(i, power)| CommitteeMember {
                    address: i as u8,
                    public_key: pk(i as u8),
                    vote_power: (*power).into(),
                })
                .collect(),
        )
    }

    fn skip_set(members: &[u8]) -> LeaderSkipSet {
        LeaderSkipSet {
            skipped: members.iter().map(|i| pk(*i)).collect(),
        }
    }

    #[test]
    fn an_empty_set_is_plain_round_robin() {
        let committee = committee(4);
        let set = LeaderSkipSet::none();
        for height in 0..8u64 {
            let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &committee, NodeHeight(height));
            assert_eq!(*addr, (height % 4) as u8);
        }
    }

    #[test]
    fn a_skipped_leader_hands_the_view_to_the_next_one() {
        let committee = committee(4);
        let set = skip_set(&[1]);
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &committee, NodeHeight(1));
        assert_eq!(*addr, 2);
        // The views around it are untouched.
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &committee, NodeHeight(2));
        assert_eq!(*addr, 2);
    }

    #[test]
    fn the_walk_passes_over_adjacent_skipped_leaders() {
        let committee = committee(4);
        let set = skip_set(&[1, 2]);
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &committee, NodeHeight(1));
        assert_eq!(*addr, 3);
    }

    #[test]
    fn the_walk_wraps_around_the_committee() {
        let committee = committee(4);
        let set = skip_set(&[3, 0]);
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &committee, NodeHeight(3));
        assert_eq!(*addr, 1);
    }

    #[test]
    fn everyone_skipped_falls_back_to_the_round_robin_leader() {
        let committee = committee(4);
        let set = skip_set(&[0, 1, 2, 3]);
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &committee, NodeHeight(2));
        assert_eq!(*addr, 2);
    }

    #[test]
    fn the_walk_wraps_past_the_top_of_the_height_range() {
        // u64::MAX % 4 == 3
        let four = committee(4);
        let set = skip_set(&[3]);
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &four, NodeHeight::max());
        assert_eq!(*addr, 0);

        // u64::MAX % 3 == 0, and height zero after the wrap lands on the same skipped member.
        let three = committee(3);
        let set = skip_set(&[0]);
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &three, NodeHeight::max());
        assert_eq!(*addr, 1);
    }

    #[test]
    fn the_walk_reaches_past_a_skipped_heavy_leader() {
        // Schedule: [0, 0, 0, 1, 0, 0, 2, 0, 0, 0]. Member 0 holds the first three slots, so a walk
        // bounded by the committee size would never leave it.
        let committee = weighted_committee(&[8, 1, 1]);
        let set = skip_set(&[0]);
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &committee, NodeHeight(0));
        assert_eq!(*addr, 1);
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &committee, NodeHeight(4));
        assert_eq!(*addr, 2);
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &committee, NodeHeight(7));
        assert_eq!(*addr, 1);
    }

    #[test]
    fn a_skipped_light_leader_hands_the_view_to_the_next_slot() {
        let committee = weighted_committee(&[8, 1, 1]);
        let set = skip_set(&[1]);
        let (addr, _) = set.effective_leader(&SmoothWeightedRoundRobin, &committee, NodeHeight(3));
        assert_eq!(*addr, 0);
    }
}
