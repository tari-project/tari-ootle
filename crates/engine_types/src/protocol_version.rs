// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{self, Display};

use ootle_network::Network;
use serde::{Deserialize, Serialize};

use crate::Epoch;

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, borsh::BorshSerialize)]
#[borsh(use_discriminant = true)]
pub enum ProtocolVersion {
    V0 = 0,
}

impl ProtocolVersion {
    /// The schema activation schedule for `network`, ordered by activation epoch ascending. Entry at
    /// index 0 is the genesis schema, which every network starts under.
    ///
    /// Networks run at independent epochs, so an activation is scheduled per network: the epoch at
    /// which a schema goes live on esmeralda says nothing about when it goes live on igor.
    ///
    /// NB: entries here are CONSENSUS-BOUND via `hash_substate`. Never reorder or mutate an entry
    /// after it has activated on a live network — doing so changes every hash derived under it. The
    /// match is exhaustive so that a new network must state its own schedule rather than inherit one.
    const fn activations(network: Network) -> &'static [(Epoch, Self)] {
        match network {
            Network::MainNet => &[(Epoch(0), Self::V0)],
            Network::StageNet => &[(Epoch(0), Self::V0)],
            Network::NextNet => &[(Epoch(0), Self::V0)],
            Network::Igor => &[(Epoch(0), Self::V0)],
            Network::Esmeralda => &[(Epoch(0), Self::V0)],
            Network::LocalNet => &[(Epoch(0), Self::V0)],
        }
    }

    pub fn at(network: Network, epoch: Epoch) -> Self {
        Self::activations(network)
            .iter()
            .rev()
            .find(|(at, _)| *at <= epoch)
            .map(|(_, v)| *v)
            .expect("activation schedule is never empty")
    }

    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    /// The newest activation after genesis on `network`, if its schedule has one beyond the genesis
    /// schema.
    pub fn newest_scheduled_activation(network: Network) -> Option<(Epoch, Self)> {
        Self::activations(network)[1..].last().copied()
    }

    /// Guards against starting a binary that introduces a schema activation at an epoch this node has
    /// already run past.
    ///
    /// `hash_substate` selects the schema from the epoch a substate was *created* at, so an activation
    /// landing on epochs that already hold committed substates silently re-hashes them: state sync
    /// recomputes roots that no longer match the quorum-signed ones, substate proofs fail to verify, and
    /// upgraded nodes diverge from the rest rather than stopping. None of that surfaces as an error at
    /// the point it goes wrong, which is why it is caught here instead.
    ///
    /// `recorded` is the newest activation this node last started with and `last_known_epoch` the epoch
    /// it last observed, both read from the global metadata store. The comparison is against the
    /// *schedule*, not the epoch alone, so restarting after a correctly scheduled activation is not a
    /// finding. A node with no epoch history has no state to invalidate and always passes.
    ///
    /// Returns the activation to record for the next start.
    pub fn check_activation_schedule(
        network: Network,
        recorded: Option<Epoch>,
        last_known_epoch: Option<Epoch>,
    ) -> Result<Option<Epoch>, PastActivationError> {
        Self::check_schedule(
            Self::newest_scheduled_activation(network).map(|(epoch, _)| epoch),
            recorded,
            last_known_epoch,
        )
    }

    /// [`Self::check_activation_schedule`] against an explicit schedule, so the rule can be exercised
    /// for schedules other than the one this binary is compiled with.
    fn check_schedule(
        newest: Option<Epoch>,
        recorded: Option<Epoch>,
        last_known_epoch: Option<Epoch>,
    ) -> Result<Option<Epoch>, PastActivationError> {
        if newest == recorded {
            return Ok(recorded);
        }
        if let (Some(activation), Some(last_known_epoch)) = (newest, last_known_epoch) &&
            activation <= last_known_epoch
        {
            return Err(PastActivationError {
                activation,
                last_known_epoch,
            });
        }
        Ok(newest)
    }
}

/// This binary schedules a schema activation at an epoch the node has already passed. See
/// [`ProtocolVersion::check_activation_schedule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "Protocol schema activates at epoch {activation}, which this node has already passed (last known epoch \
     {last_known_epoch}). Starting would re-hash committed state and diverge from the network."
)]
pub struct PastActivationError {
    pub activation: Epoch,
    pub last_known_epoch: Epoch,
}

impl Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "V{}", self.as_u32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NETWORKS: [Network; 6] = [
        Network::MainNet,
        Network::StageNet,
        Network::NextNet,
        Network::Igor,
        Network::Esmeralda,
        Network::LocalNet,
    ];

    #[test]
    fn every_network_starts_at_v0() {
        for network in NETWORKS {
            assert_eq!(ProtocolVersion::at(network, Epoch(0)), ProtocolVersion::V0, "{network}");
        }
    }

    #[test]
    fn far_future_resolves_to_the_newest_activation() {
        for network in NETWORKS {
            let (_, newest) = *ProtocolVersion::activations(network).last().unwrap();
            assert_eq!(ProtocolVersion::at(network, Epoch(u64::MAX)), newest, "{network}");
        }
    }

    #[test]
    fn genesis_is_never_a_scheduled_activation() {
        // Genesis is the schema every network starts under, so no node can have run past it.
        for network in NETWORKS {
            let genesis = ProtocolVersion::activations(network)[0];
            assert_eq!(genesis, (Epoch(0), ProtocolVersion::V0), "{network}");
        }
    }

    #[test]
    fn an_unchanged_schedule_is_not_rechecked() {
        // The recorded activation is behind the node's epoch, which is the normal state of affairs
        // after an activation has been honoured.
        assert_eq!(
            ProtocolVersion::check_schedule(Some(Epoch(100)), Some(Epoch(100)), Some(Epoch(900))),
            Ok(Some(Epoch(100)))
        );
    }

    #[test]
    fn a_new_activation_ahead_of_the_node_is_recorded() {
        assert_eq!(
            ProtocolVersion::check_schedule(Some(Epoch(1000)), None, Some(Epoch(900))),
            Ok(Some(Epoch(1000)))
        );
    }

    #[test]
    fn a_new_activation_the_node_has_passed_is_rejected() {
        assert_eq!(
            ProtocolVersion::check_schedule(Some(Epoch(900)), None, Some(Epoch(900))),
            Err(PastActivationError {
                activation: Epoch(900),
                last_known_epoch: Epoch(900),
            })
        );
        assert!(ProtocolVersion::check_schedule(Some(Epoch(100)), None, Some(Epoch(900))).is_err());
    }

    #[test]
    fn a_node_with_no_epoch_history_has_no_state_to_invalidate() {
        assert_eq!(
            ProtocolVersion::check_schedule(Some(Epoch(1)), None, None),
            Ok(Some(Epoch(1)))
        );
    }

    #[test]
    fn monotonic_across_activations() {
        for network in NETWORKS {
            let mut prev: Option<Epoch> = None;
            for (at, _) in ProtocolVersion::activations(network) {
                if let Some(p) = prev {
                    assert!(*at >= p, "{network} schedule must be sorted ascending by epoch");
                }
                prev = Some(*at);
            }
        }
    }
}
