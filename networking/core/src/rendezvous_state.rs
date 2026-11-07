//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use libp2p::Multiaddr;
use tari_swarm::rendezvous::Cookie;

use crate::PeerId;

/// Tracks the rendezvous servers this node uses for peer discovery.
///
/// The rendezvous servers are assumed to be the configured seed peers (those with a known peer id). This is
/// best-effort: a seed peer that does not run a rendezvous server simply yields failed (and harmless) register/discover
/// attempts.
#[derive(Debug, Default)]
pub struct RendezvousState {
    servers: HashMap<PeerId, Vec<RvPeerState>>,
}

#[derive(Debug, Clone)]
struct RvPeerState {
    address: Multiaddr,
    cookie: Option<Cookie>,
}

impl RendezvousState {
    pub fn new<I: IntoIterator<Item = (PeerId, Multiaddr)>>(servers: I) -> Self {
        let mut map = HashMap::<PeerId, Vec<RvPeerState>>::new();
        for (peer_id, address) in servers {
            let entries = map.entry(peer_id).or_default();
            if !entries.iter().any(|s| s.address == address) {
                entries.push(RvPeerState { address, cookie: None });
            }
        }
        Self { servers: map }
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    /// Returns true if the given peer is a configured rendezvous server.
    pub fn contains(&self, peer_id: &PeerId) -> bool {
        self.servers.contains_key(peer_id)
    }

    /// Stores the pagination cookie from a discover response for the given server. Returns false if the peer is not a
    /// known rendezvous server.
    pub fn set_cookie(&mut self, peer_id: &PeerId, cookie: Cookie) -> bool {
        match self.servers.get_mut(peer_id) {
            Some(states) => {
                for state in states {
                    state.cookie = Some(cookie.clone());
                }
                true
            },
            None => false,
        }
    }

    /// Returns the (peer id, addresses, last cookie) of each configured rendezvous server.
    pub fn servers(&self) -> impl Iterator<Item = (PeerId, Vec<Multiaddr>, Option<Cookie>)> + '_ {
        self.servers.iter().map(|(peer_id, states)| {
            let addresses = states.iter().map(|s| s.address.clone()).collect();
            let cookie = states.first().and_then(|s| s.cookie.clone());
            (*peer_id, addresses, cookie)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_all_addresses_for_same_rendezvous_server() {
        let peer = PeerId::random();
        let addr1: Multiaddr = "/ip4/1.1.1.1/tcp/1000".parse().unwrap();
        let addr2: Multiaddr = "/ip4/1.1.1.1/udp/1000/quic-v1".parse().unwrap();

        let state = RendezvousState::new([(peer, addr1.clone()), (peer, addr2.clone()), (peer, addr1.clone())]);
        assert!(state.contains(&peer));

        let servers: Vec<_> = state.servers().collect();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].0, peer);
        assert_eq!(servers[0].1, vec![addr1, addr2]);
        assert!(servers[0].2.is_none());
    }
}
