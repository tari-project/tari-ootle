//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Boundary [`Network`] enum.
//!
//! A separate boundary enum (rather than re-exposing the internal one) keeps the boundary surface
//! free of internal derives/attributes; `From`/`Into` convert between them. serde uses the same
//! lowercase keys as the internal type so fixtures are interchangeable.

use ootle_network::Network as InternalNetwork;
use serde::{Deserialize, Serialize};

/// The Tari networks, boundary form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    /// Production mainnet.
    MainNet,
    /// Stagenet.
    StageNet,
    /// Nextnet.
    NextNet,
    /// Local development network.
    LocalNet,
    /// Igor testnet.
    Igor,
    /// Esmeralda testnet (default).
    #[default]
    Esmeralda,
}

impl Network {
    /// The L1-compatible network discriminant byte.
    pub fn as_byte(self) -> u8 {
        InternalNetwork::from(self).as_byte()
    }
}

impl From<Network> for InternalNetwork {
    fn from(n: Network) -> Self {
        match n {
            Network::MainNet => InternalNetwork::MainNet,
            Network::StageNet => InternalNetwork::StageNet,
            Network::NextNet => InternalNetwork::NextNet,
            Network::LocalNet => InternalNetwork::LocalNet,
            Network::Igor => InternalNetwork::Igor,
            Network::Esmeralda => InternalNetwork::Esmeralda,
        }
    }
}

impl From<InternalNetwork> for Network {
    fn from(n: InternalNetwork) -> Self {
        match n {
            InternalNetwork::MainNet => Network::MainNet,
            InternalNetwork::StageNet => Network::StageNet,
            InternalNetwork::NextNet => Network::NextNet,
            InternalNetwork::LocalNet => Network::LocalNet,
            InternalNetwork::Igor => Network::Igor,
            InternalNetwork::Esmeralda => Network::Esmeralda,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants_round_trip_to_internal() {
        for n in [
            Network::MainNet,
            Network::StageNet,
            Network::NextNet,
            Network::LocalNet,
            Network::Igor,
            Network::Esmeralda,
        ] {
            let internal: InternalNetwork = n.into();
            assert_eq!(Network::from(internal), n);
            assert_eq!(n.as_byte(), internal.as_byte());
        }
    }

    #[test]
    fn default_is_esmeralda() {
        assert_eq!(Network::default(), Network::Esmeralda);
    }

    #[test]
    fn serde_uses_lowercase_keys() {
        assert_eq!(serde_json::to_string(&Network::Esmeralda).unwrap(), "\"esmeralda\"");
        assert_eq!(serde_json::to_string(&Network::MainNet).unwrap(), "\"mainnet\"");
    }
}
