// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! The state the on-chain burn rate governance component holds, shared by the template that writes
//! it and the consensus code that reads it.
//!
//! Both sides use these types rather than mirroring each other, so the encoding cannot drift: the
//! template's component state is [`BurnRateGovernanceState`] field for field, and consensus decodes
//! the substate back into it.
//!
//! The council itself is not in here. It is the component's owner rule — an m-of-n over the members'
//! public identity badges, which every transaction signer contributes to the authorization scope —
//! so the engine evaluates it natively and anything that can read the component can read who may
//! act. [`council_owner_rule`] builds it.

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::prelude::*;

use crate::{
    access_rules::{AccessRule, RequireRule, RestrictedAccessRule, RuleRequirement},
    crypto::RistrettoPublicKeyBytes,
    owner_rule::SubstateOwnerRule,
};

/// The highest exhaust burn rate the network can be set to, in basis points: the whole of what a
/// transaction paid.
///
/// The burn is a share of the fees collected, so a rate is meaningful only up to `10_000` — every
/// microtari paid is burned and leaders receive nothing. The user's price is the fee table alone
/// whatever the rate; the rate only splits what was collected between leaders and the burn.
pub const MAX_EXHAUST_BURN_RATE_BPS: u16 = 10_000;

/// The fewest epochs between the epoch a council transaction executes in and the epoch its rate may
/// take effect in.
///
/// A rate takes effect at an epoch boundary, and the transaction that schedules it commits per shard
/// group rather than atomically across the network. Two epochs is what keeps a transaction that
/// straddles a boundary from being seen by one shard group before the boundary and another after it,
/// which would leave the groups disagreeing about the rate the next epoch opens at.
pub const MIN_BURN_RATE_ACTIVATION_LEAD_EPOCHS: u64 = 2;

/// One scheduled change to the exhaust burn rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BurnRateChange {
    /// The first epoch this rate applies to.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    #[n(0)]
    pub activation_epoch: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    #[n(1)]
    pub rate_bps: u16,
}

/// The body of the burn rate governance component.
#[derive(Debug, Clone, Default, PartialEq, Eq, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BurnRateGovernanceState {
    /// Ascending by activation epoch, with at most one entry per epoch.
    ///
    /// Absolute rather than a sequence of deltas: the rate for an epoch is the newest entry that has
    /// activated by it, so it can be read without knowing the rate any earlier epoch ran at. That is
    /// what lets a node which joined by state sync resolve the same rate as one that has been running
    /// since genesis.
    ///
    /// Empty means the release-scheduled table governs, which covers a network whose council has
    /// never acted.
    #[n(0)]
    pub schedule: Vec<BurnRateChange>,
    /// The first epoch the release-scheduled table governs again because the council retired.
    ///
    /// Retirement is an activation like any other, held to the same lead time, so every epoch that
    /// could already have been resolved keeps the rate the schedule gave it. The schedule is left in
    /// place for the epochs before this one.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    #[n(1)]
    pub retired_from: Option<u64>,
}

impl BurnRateGovernanceState {
    pub const fn new() -> Self {
        Self {
            schedule: Vec::new(),
            retired_from: None,
        }
    }

    /// The rate in force at `epoch`: the newest entry that has activated by it, or `None` if the
    /// council has scheduled nothing that reaches `epoch` or has retired by it.
    pub fn rate_at(&self, epoch: u64) -> Option<u16> {
        if self.retired_from.is_some_and(|retired_from| epoch >= retired_from) {
            return None;
        }
        rate_at(&self.schedule, epoch)
    }
}

/// The owner rule that admits a transaction at least `threshold` of `council` signed.
///
/// Every signer of a transaction contributes its public identity badge to the authorization scope, so
/// an m-of-n over those badges is satisfied by the signatures on the transaction. No badge resource
/// is minted, held in a vault or passed to a method.
///
/// Panics if the engine would reject the rule it builds — a threshold of zero admits everyone and one
/// above the council admits nobody, and neither is a council anyone means to seat. The engine applies
/// the same check to a rule reaching it through `SetOwnerRule`; this one covers genesis, which writes
/// the component straight to the state store.
pub fn council_owner_rule(threshold: u16, council: &[RistrettoPublicKeyBytes]) -> SubstateOwnerRule {
    let rule = AccessRule::Restricted(RestrictedAccessRule::Require(RequireRule::MOfN(
        threshold,
        council.iter().copied().map(RuleRequirement::from).collect(),
    )));

    if let Some(invalid) = rule.find_invalid_m_of_n() {
        panic!(
            "a council of {} cannot be seated at a threshold of {}",
            invalid.num_requirements, invalid.threshold
        );
    }

    SubstateOwnerRule::ByAccessRule(rule)
}

/// The owner rule of a component that governs nothing: no caller satisfies it, and because the engine
/// gates `SetOwnerRule` on the current rule, nothing can ever replace it.
///
/// This is both a network that seats no council and a council that has retired.
pub const fn no_council_owner_rule() -> SubstateOwnerRule {
    SubstateOwnerRule::None
}

/// The rate `schedule` puts in force at `epoch`.
pub fn rate_at(schedule: &[BurnRateChange], epoch: u64) -> Option<u16> {
    schedule
        .iter()
        .rev()
        .find(|change| change.activation_epoch <= epoch)
        .map(|change| change.rate_bps)
}

/// Records `change` in `schedule`, replacing an entry that already activates at the same epoch and
/// keeping the schedule ascending.
///
/// Rescheduling an activation that has not happened yet is the supported way to abort one, so a later
/// entry does not stop an earlier epoch being rescheduled.
pub fn schedule_change(schedule: &mut Vec<BurnRateChange>, change: BurnRateChange) {
    match schedule.binary_search_by(|existing| existing.activation_epoch.cmp(&change.activation_epoch)) {
        Ok(index) => schedule[index] = change,
        Err(index) => schedule.insert(index, change),
    }
}

/// Drops every entry in `schedule` that activates before `epoch`, except the one in force at it.
///
/// The schedule is component state, so it is read, decoded and re-persisted on every council
/// transaction. Keeping only what can still be asked for bounds that cost however long the council
/// governs.
pub fn prune_before(schedule: &mut Vec<BurnRateChange>, epoch: u64) {
    let Some(in_force) = schedule.iter().rposition(|change| change.activation_epoch <= epoch) else {
        return;
    };
    schedule.drain(..in_force);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(activation_epoch: u64, rate_bps: u16) -> BurnRateChange {
        BurnRateChange {
            activation_epoch,
            rate_bps,
        }
    }

    fn schedule(changes: &[(u64, u16)]) -> Vec<BurnRateChange> {
        let mut out = Vec::new();
        for (epoch, bps) in changes {
            schedule_change(&mut out, change(*epoch, *bps));
        }
        out
    }

    fn council(size: u8) -> Vec<RistrettoPublicKeyBytes> {
        (0..size)
            .map(|i| RistrettoPublicKeyBytes::from_bytes(&[i; 32]).unwrap())
            .collect()
    }

    #[test]
    fn an_empty_schedule_governs_nothing() {
        assert_eq!(BurnRateGovernanceState::new().rate_at(0), None);
        assert_eq!(BurnRateGovernanceState::new().rate_at(u64::MAX), None);
    }

    #[test]
    fn an_epoch_before_the_first_activation_is_not_governed() {
        assert_eq!(rate_at(&schedule(&[(10, 700)]), 9), None);
    }

    #[test]
    fn an_epoch_reads_the_newest_activation_at_or_before_it() {
        let schedule = schedule(&[(10, 700), (20, 900)]);
        assert_eq!(rate_at(&schedule, 10), Some(700));
        assert_eq!(rate_at(&schedule, 19), Some(700));
        assert_eq!(rate_at(&schedule, 20), Some(900));
        assert_eq!(rate_at(&schedule, u64::MAX), Some(900));
    }

    #[test]
    fn entries_are_kept_ascending_whatever_order_they_arrive_in() {
        assert_eq!(schedule(&[(20, 900), (10, 700), (30, 100)]), vec![
            change(10, 700),
            change(20, 900),
            change(30, 100)
        ]);
    }

    #[test]
    fn rescheduling_an_epoch_replaces_it_rather_than_duplicating_it() {
        let schedule = schedule(&[(10, 700), (20, 900), (10, 50)]);
        assert_eq!(schedule, vec![change(10, 50), change(20, 900)]);
        assert_eq!(rate_at(&schedule, 10), Some(50));
    }

    #[test]
    fn pruning_keeps_the_entry_in_force_and_everything_ahead_of_it() {
        let mut schedule = schedule(&[(10, 700), (20, 900), (30, 100)]);
        prune_before(&mut schedule, 25);
        assert_eq!(schedule, vec![change(20, 900), change(30, 100)]);
        assert_eq!(rate_at(&schedule, 25), Some(900));
    }

    #[test]
    fn a_retired_council_governs_only_the_epochs_before_retirement() {
        let state = BurnRateGovernanceState {
            schedule: schedule(&[(10, 700), (20, 900)]),
            retired_from: Some(15),
        };
        assert_eq!(state.rate_at(9), None);
        assert_eq!(state.rate_at(14), Some(700));
        assert_eq!(state.rate_at(15), None);
        assert_eq!(state.rate_at(20), None);
    }

    #[test]
    fn pruning_before_the_first_activation_keeps_everything() {
        let mut schedule = schedule(&[(10, 700), (20, 900)]);
        prune_before(&mut schedule, 5);
        assert_eq!(schedule, vec![change(10, 700), change(20, 900)]);
    }

    #[test]
    fn a_council_owner_rule_requires_the_threshold_over_every_member() {
        let members = council(3);
        let SubstateOwnerRule::ByAccessRule(AccessRule::Restricted(RestrictedAccessRule::Require(RequireRule::MOfN(
            threshold,
            requirements,
        )))) = council_owner_rule(2, &members)
        else {
            panic!("a council owner rule is an m-of-n require rule");
        };
        assert_eq!(threshold, 2);
        assert_eq!(
            requirements.into_vec(),
            members.into_iter().map(RuleRequirement::from).collect::<Vec<_>>()
        );
    }

    /// The engine rejects these through `find_invalid_m_of_n`, and genesis never reaches the engine,
    /// so the same check has to hold here.
    #[test]
    #[should_panic(expected = "a council of 3 cannot be seated at a threshold of 0")]
    fn a_zero_threshold_is_rejected() {
        council_owner_rule(0, &council(3));
    }

    #[test]
    #[should_panic(expected = "a council of 3 cannot be seated at a threshold of 4")]
    fn a_threshold_larger_than_the_council_is_rejected() {
        council_owner_rule(4, &council(3));
    }

    #[test]
    fn no_council_is_satisfied_by_nobody() {
        assert_eq!(no_council_owner_rule(), SubstateOwnerRule::None);
    }
}
