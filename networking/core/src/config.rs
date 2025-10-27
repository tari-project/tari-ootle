//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use libp2p::Multiaddr;

#[derive(Debug, Clone)]
pub struct Config {
    pub swarm: tari_swarm::Config,
    pub listeners: Vec<Multiaddr>,
    pub reachability_mode: ReachabilityMode,
    pub announce: bool,
    pub check_connections_interval: Duration,
    pub known_local_public_address: Vec<Multiaddr>,
    /// The namespace used for rendezvous service registration.
    /// This MUST be less than 255 characters.
    pub rendezvous_namespace: String,
    pub rendezvous_peer_limit: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            swarm: tari_swarm::Config::default(),
            listeners: vec![
                "/ip4/0.0.0.0/tcp/0".parse().expect("Invalid multiaddr"),
                "/ip4/0.0.0.0/udp/0/quic-v1".parse().expect("Invalid multiaddr"),
            ],
            reachability_mode: ReachabilityMode::default(),
            announce: false,
            check_connections_interval: Duration::from_secs(2 * 60),
            known_local_public_address: vec![],
            rendezvous_namespace: "tari".to_string(),
            rendezvous_peer_limit: Some(1000),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum ReachabilityMode {
    #[default]
    Auto,
    Private,
}

impl ReachabilityMode {
    pub fn is_private(&self) -> bool {
        matches!(self, ReachabilityMode::Private)
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, ReachabilityMode::Auto)
    }
}
