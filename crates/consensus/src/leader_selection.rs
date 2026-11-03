//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{NodeHeight, committee::Committee};

use crate::traits::LeaderStrategy;

/// Smooth weighted round robin over the committee in committee order.
///
/// The schedule repeats every `total_power` heights and gives each member exactly `vote_power` of
/// them, spreading a heavy member's slots through the period rather than bunching them. Each slot
/// adds every member's power to its counter, picks the highest counter (the lowest index on a tie)
/// and takes `total_power` off the winner's counter.
///
/// At equal powers the schedule is `height mod len`: on every step all counters but the previous
/// winners' are tied, so the lowest index that has not yet led wins.
///
/// A call simulates the schedule up to the slot it asks about, costing up to `total_power × len`
/// steps, so powers are expected to be small integers rather than raw stake amounts.
#[derive(Debug, Clone, Copy, Default)]
pub struct SmoothWeightedRoundRobin;

impl SmoothWeightedRoundRobin {
    pub fn new() -> Self {
        Self
    }
}

impl<TAddr: PartialEq> LeaderStrategy<TAddr> for SmoothWeightedRoundRobin {
    /// Panics on an empty committee. A committee whose members all have zero power has no weights
    /// to follow and falls back to `height mod len`.
    fn calculate_leader(&self, committee: &Committee<TAddr>, height: NodeHeight) -> u32 {
        let total_power = committee.total_power().value();
        if total_power == 0 {
            return (height.as_u64() % committee.len() as u64) as u32;
        }

        // Counters go negative and stay within `len × total_power` of zero, so they need a signed type wider than u64.
        let total_power_i128 = i128::from(total_power);
        let mut counters = vec![0i128; committee.len()];
        let mut leader = 0usize;
        for _ in 0..=height.as_u64() % total_power {
            let mut highest = None;
            for (index, member) in committee.iter().enumerate() {
                // A zero-power member's counter stays at zero, which can tie the maximum and would win the
                // lowest-index tie-break.
                if member.vote_power.is_zero() {
                    continue;
                }
                counters[index] += i128::from(member.vote_power.value());
                if highest.is_none_or(|best: usize| counters[index] > counters[best]) {
                    highest = Some(index);
                }
            }
            leader = highest.expect("a committee with non-zero total power has a member with non-zero power");
            counters[leader] -= total_power_i128;
        }

        leader as u32
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::committee::CommitteeMember;
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

    use super::*;

    /// Builds a committee whose member at index `i` has address `i` and power `powers[i]`.
    fn committee(powers: &[u64]) -> Committee<u8> {
        Committee::new(
            powers
                .iter()
                .enumerate()
                .map(|(i, power)| {
                    let mut bytes = [0u8; 32];
                    bytes[0] = i as u8;
                    CommitteeMember {
                        address: i as u8,
                        public_key: RistrettoPublicKeyBytes::from_bytes(&bytes).unwrap(),
                        vote_power: (*power).into(),
                    }
                })
                .collect(),
        )
    }

    fn schedule(committee: &Committee<u8>) -> Vec<u32> {
        (0..committee.total_power().value())
            .map(|height| SmoothWeightedRoundRobin.calculate_leader(committee, NodeHeight(height)))
            .collect()
    }

    #[test]
    fn equal_powers_match_height_mod_committee_size() {
        for size in 1..=64u64 {
            let committee = committee(&vec![1; size as usize]);
            let heights = (0..3 * size).chain([u64::MAX - 1, u64::MAX, 1_000_003, 987_654_321_123]);
            for height in heights {
                assert_eq!(
                    SmoothWeightedRoundRobin.calculate_leader(&committee, NodeHeight(height)),
                    (height % size) as u32,
                    "size {size}, height {height}"
                );
            }
        }
    }

    #[test]
    fn equal_powers_above_one_match_height_mod_committee_size() {
        let committee = committee(&[3, 3, 3, 3]);
        for height in 0..24u64 {
            assert_eq!(
                SmoothWeightedRoundRobin.calculate_leader(&committee, NodeHeight(height)),
                (height % 4) as u32
            );
        }
    }

    #[test]
    fn each_member_leads_its_power_times_per_period() {
        for powers in [&[8, 1, 1][..], &[3, 2, 1], &[1, 5, 2, 7], &[10, 10, 1], &[
            2, 0, 3, 0, 1,
        ]] {
            let committee = committee(powers);
            let schedule = schedule(&committee);
            for (index, power) in powers.iter().enumerate() {
                let slots = schedule.iter().filter(|leader| **leader == index as u32).count();
                assert_eq!(slots as u64, *power, "powers {powers:?}, member {index}");
            }
        }
    }

    #[test]
    fn the_schedule_repeats_every_total_power_heights() {
        let committee = committee(&[3, 2, 1]);
        for height in 0..60u64 {
            assert_eq!(
                SmoothWeightedRoundRobin.calculate_leader(&committee, NodeHeight(height)),
                SmoothWeightedRoundRobin.calculate_leader(&committee, NodeHeight(height + 6))
            );
        }
    }

    #[test]
    fn a_heavy_member_has_its_slots_interleaved() {
        assert_eq!(schedule(&committee(&[8, 1, 1])), [0, 0, 0, 1, 0, 0, 2, 0, 0, 0]);
        assert_eq!(schedule(&committee(&[3, 2, 1])), [0, 1, 0, 2, 1, 0]);
        assert_eq!(schedule(&committee(&[1, 1, 4])), [2, 0, 2, 1, 2, 2]);
    }

    #[test]
    fn a_zero_power_member_never_leads() {
        for powers in [&[0, 1, 1][..], &[0, 3, 1], &[1, 0, 1], &[2, 1, 0], &[0, 0, 1]] {
            let committee = committee(powers);
            for height in 0..3 * committee.total_power().value() {
                let leader = SmoothWeightedRoundRobin.calculate_leader(&committee, NodeHeight(height));
                assert_ne!(powers[leader as usize], 0, "powers {powers:?}, height {height}");
            }
        }
    }

    #[test]
    fn an_all_zero_power_committee_falls_back_to_height_mod_committee_size() {
        let committee = committee(&[0, 0, 0]);
        for height in 0..9u64 {
            assert_eq!(
                SmoothWeightedRoundRobin.calculate_leader(&committee, NodeHeight(height)),
                (height % 3) as u32
            );
        }
    }
}
