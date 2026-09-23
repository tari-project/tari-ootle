// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! Where an epoch's exhaust burn rate comes from, and the bounds every source is held to.
//!
//! The rate a transaction is settled at is never read from here at execution time. It is resolved
//! once, when the end-of-epoch block that opens the next epoch is proposed, ratified by the quorum
//! that commits that block, and then carried in every block header of the epoch it governs. This
//! module is the pure part of that resolution: given an epoch and whatever the active source asks
//! for, it says what that epoch runs at.

use ootle_network::Network;

use crate::{Epoch, fees::ExhaustBurnRate};

/// Where the rate for an epoch is resolved from.
///
/// The active source is a per-network activation schedule ([`Self::at`]) rather than a constant, so
/// a network can be moved back to [`Self::Table`] in a release if the source above it is unavailable
/// or wrong. That is the break-glass path, which is why the table never retires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExhaustBurnRateSource {
    /// The release-scheduled rate in [`ExhaustBurnRateSchedule`]. Changing the rate means shipping a
    /// binary, so this is the floor of the design rather than its goal.
    Table,
    /// The rate the on-chain governance component holds, falling back to [`Self::Table`] for as long
    /// as the council has scheduled nothing.
    Governance,
}

impl ExhaustBurnRateSource {
    /// The source schedule for `network`, ordered by activation epoch ascending. Entry at index 0 is
    /// the source the network starts under.
    ///
    /// The match is exhaustive so that a new network must state its own schedule rather than inherit
    /// one.
    const fn activations(network: Network) -> &'static [(Epoch, Self)] {
        match network {
            Network::MainNet => &[(Epoch(0), Self::Governance)],
            Network::StageNet => &[(Epoch(0), Self::Governance)],
            Network::NextNet => &[(Epoch(0), Self::Governance)],
            Network::Igor => &[(Epoch(0), Self::Governance)],
            Network::Esmeralda => &[(Epoch(0), Self::Governance)],
            Network::LocalNet => &[(Epoch(0), Self::Governance)],
        }
    }

    pub fn at(network: Network, epoch: Epoch) -> Self {
        Self::activations(network)
            .iter()
            .rev()
            .find(|(at, _)| *at <= epoch)
            .map(|(_, source)| *source)
            .expect("source schedule is never empty")
    }
}

/// The release-scheduled exhaust burn rate for each network.
pub struct ExhaustBurnRateSchedule;

impl ExhaustBurnRateSchedule {
    /// The rate schedule for `network`, ordered by activation epoch ascending. Entry at index 0 is
    /// the rate the network starts under, and is what the genesis epoch is latched at.
    ///
    /// Networks run at independent epochs, so an entry is scheduled per network: the epoch at which
    /// a rate goes live on esmeralda says nothing about when it goes live on igor.
    const fn activations(network: Network) -> &'static [(Epoch, ExhaustBurnRate)] {
        // Named consts rather than inline arrays: `ExhaustBurnRate::new` asserts, so an array built
        // inline is a temporary rather than a promoted `'static`.
        const MAINNET: &[(Epoch, ExhaustBurnRate)] = &[(Epoch(0), ExhaustBurnRate::new(500))];
        const STAGENET: &[(Epoch, ExhaustBurnRate)] = &[(Epoch(0), ExhaustBurnRate::new(500))];
        const NEXTNET: &[(Epoch, ExhaustBurnRate)] = &[(Epoch(0), ExhaustBurnRate::new(500))];
        const IGOR: &[(Epoch, ExhaustBurnRate)] = &[(Epoch(0), ExhaustBurnRate::new(500))];
        const ESMERALDA: &[(Epoch, ExhaustBurnRate)] = &[(Epoch(0), ExhaustBurnRate::new(500))];
        const LOCALNET: &[(Epoch, ExhaustBurnRate)] = &[(Epoch(0), ExhaustBurnRate::new(500))];

        match network {
            Network::MainNet => MAINNET,
            Network::StageNet => STAGENET,
            Network::NextNet => NEXTNET,
            Network::Igor => IGOR,
            Network::Esmeralda => ESMERALDA,
            Network::LocalNet => LOCALNET,
        }
    }

    /// The rate `network` starts under, which the genesis epoch is latched at.
    pub fn genesis(network: Network) -> ExhaustBurnRate {
        Self::activations(network)[0].1
    }

    pub fn at(network: Network, epoch: Epoch) -> ExhaustBurnRate {
        Self::activations(network)
            .iter()
            .rev()
            .find(|(at, _)| *at <= epoch)
            .map(|(_, rate)| *rate)
            .expect("rate schedule is never empty")
    }
}

/// The rate that takes effect at `epoch`, given what the governance component holds for it.
///
/// `governance` is [`BurnRateGovernanceState::rate_at`](crate) read for `epoch`: `None` when the
/// council has scheduled nothing that reaches `epoch`, and equally when the component is absent. Both
/// fall back to [`ExhaustBurnRateSchedule`], which is why a network whose council has never acted
/// still resolves a rate every epoch.
///
/// The result depends only on `epoch` and on state every shard group holds, never on the rate an
/// earlier epoch ran at. That is what lets a node that joined by state sync, and so witnessed no
/// epoch open, resolve the same rate as one that has been running since genesis.
pub fn resolve_exhaust_burn_rate(
    network: Network,
    epoch: Epoch,
    governance: Option<ExhaustBurnRate>,
) -> ExhaustBurnRate {
    match ExhaustBurnRateSource::at(network, epoch) {
        ExhaustBurnRateSource::Table => ExhaustBurnRateSchedule::at(network, epoch),
        ExhaustBurnRateSource::Governance => governance.unwrap_or_else(|| ExhaustBurnRateSchedule::at(network, epoch)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fees::MAX_EXHAUST_BURN_RATE_BPS;

    /// Derived from `Network`'s byte encoding rather than listed, so a new variant is covered without
    /// anyone remembering to add it here.
    fn all_networks() -> Vec<Network> {
        (u8::MIN..=u8::MAX)
            .filter_map(|byte| Network::try_from(byte).ok())
            .collect()
    }

    fn rate(bps: u16) -> ExhaustBurnRate {
        ExhaustBurnRate::new(bps)
    }

    #[test]
    fn every_network_starts_at_epoch_zero_and_stays_sorted() {
        for network in all_networks() {
            assert_eq!(
                ExhaustBurnRateSchedule::activations(network)[0].0,
                Epoch(0),
                "{network}"
            );
            assert_eq!(ExhaustBurnRateSource::activations(network)[0].0, Epoch(0), "{network}");

            let mut prev = Epoch(0);
            for (at, _) in ExhaustBurnRateSchedule::activations(network) {
                assert!(*at >= prev, "{network} rate schedule must be sorted ascending by epoch");
                prev = *at;
            }
            let mut prev = Epoch(0);
            for (at, _) in ExhaustBurnRateSource::activations(network) {
                assert!(
                    *at >= prev,
                    "{network} source schedule must be sorted ascending by epoch"
                );
                prev = *at;
            }
        }
    }

    #[test]
    fn far_future_resolves_to_the_newest_entry() {
        for network in all_networks() {
            let (_, newest) = *ExhaustBurnRateSchedule::activations(network).last().unwrap();
            assert_eq!(
                ExhaustBurnRateSchedule::at(network, Epoch(u64::MAX)),
                newest,
                "{network}"
            );

            let (_, newest) = *ExhaustBurnRateSource::activations(network).last().unwrap();
            assert_eq!(ExhaustBurnRateSource::at(network, Epoch(u64::MAX)), newest, "{network}");
        }
    }

    #[test]
    fn a_council_that_has_scheduled_nothing_falls_back_to_the_table() {
        for network in all_networks() {
            for epoch in [Epoch(0), Epoch(1), Epoch(u64::MAX)] {
                assert_eq!(
                    resolve_exhaust_burn_rate(network, epoch, None),
                    ExhaustBurnRateSchedule::at(network, epoch),
                    "{network} at {epoch}"
                );
            }
        }
    }

    #[test]
    fn a_council_rate_is_taken_as_asked_wherever_governance_is_the_source() {
        for network in all_networks() {
            if ExhaustBurnRateSource::at(network, Epoch(1)) != ExhaustBurnRateSource::Governance {
                continue;
            }
            for bps in [0, 1, 500, MAX_EXHAUST_BURN_RATE_BPS] {
                assert_eq!(
                    resolve_exhaust_burn_rate(network, Epoch(1), Some(rate(bps))),
                    rate(bps),
                    "{network}"
                );
            }
        }
    }

    #[test]
    fn the_table_ignores_the_council_wherever_it_is_the_source() {
        for network in all_networks() {
            if ExhaustBurnRateSource::at(network, Epoch(1)) != ExhaustBurnRateSource::Table {
                continue;
            }
            assert_eq!(
                resolve_exhaust_burn_rate(network, Epoch(1), Some(rate(1))),
                ExhaustBurnRateSchedule::at(network, Epoch(1)),
                "{network}"
            );
        }
    }
}
