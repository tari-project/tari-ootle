// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! The council each network launches with, and the component that holds it.

use tari_ootle_transaction::Network;
use tari_template_lib::types::{
    SubstateOwnerRule,
    crypto::RistrettoPublicKeyBytes,
    governance::{council_owner_rule, no_council_owner_rule},
};

/// The threshold and membership of `network`'s burn rate council.
///
/// A council is fixed at genesis. The component that holds it lives on the global shard, and a
/// substate cannot be introduced to a chain that has already taken state roots over the shard it
/// would live on, so a network launches with the council it is going to have. Rotating it afterwards
/// is what `BurnRateGovernance::set_council` is for, and it takes the sitting council's threshold.
///
/// An empty council is a network that has decided not to govern the rate on chain: the component is
/// still instantiated, owned by nobody, and the rate comes from `ExhaustBurnRateSchedule` — the
/// release-scheduled table — instead.
pub fn genesis_council(network: Network) -> (u16, Vec<RistrettoPublicKeyBytes>) {
    match network {
        Network::MainNet |
        Network::StageNet |
        Network::NextNet |
        Network::Igor |
        Network::Esmeralda |
        Network::LocalNet => (0, Vec::new()),
    }
}

/// The owner rule `network`'s governance component is created with, which is the whole of who may
/// move the rate on that network.
pub fn genesis_governance_owner_rule(network: Network) -> SubstateOwnerRule {
    let (threshold, council) = genesis_council(network);
    if threshold == 0 {
        return no_council_owner_rule();
    }
    council_owner_rule(threshold, &council)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_networks() -> Vec<Network> {
        (u8::MIN..=u8::MAX)
            .filter_map(|byte| Network::try_from(byte).ok())
            .collect()
    }

    #[test]
    fn every_networks_council_can_meet_its_own_threshold() {
        for network in all_networks() {
            let (threshold, council) = genesis_council(network);
            assert!(
                usize::from(threshold) <= council.len(),
                "{network} names a threshold of {threshold} over a council of {}",
                council.len()
            );
        }
    }

    #[test]
    fn a_network_with_no_council_admits_nobody() {
        for network in all_networks() {
            if genesis_council(network).0 > 0 {
                continue;
            }
            assert_eq!(
                genesis_governance_owner_rule(network),
                SubstateOwnerRule::None,
                "{network}"
            );
        }
    }

    /// `council_owner_rule` panics on a threshold the engine would reject, and genesis writes the
    /// component straight to the state store, so this is what stops a network launching with a
    /// council nobody can assemble.
    #[test]
    fn every_seated_council_builds_a_rule_the_engine_accepts() {
        for network in all_networks() {
            if genesis_council(network).0 == 0 {
                continue;
            }
            let SubstateOwnerRule::ByAccessRule(rule) = genesis_governance_owner_rule(network) else {
                panic!("{network} seats a council but does not name an access rule");
            };
            assert_eq!(rule.find_invalid_m_of_n(), None, "{network}");
        }
    }
}
