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
    servers: HashMap<PeerId, RvPeerState>,
}

#[derive(Debug, Clone)]
struct RvPeerState {
    address: Multiaddr,
    cookie: Option<Cookie>,
}

impl RendezvousState {
    pub fn new<I: IntoIterator<Item = (PeerId, Multiaddr)>>(servers: I) -> Self {
        Self {
            servers: servers
                .into_iter()
                .map(|(peer_id, address)| (peer_id, RvPeerState { address, cookie: None }))
                .collect(),
        }
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
            Some(state) => {
                state.cookie = Some(cookie);
                true
            },
            None => false,
        }
    }

    /// Returns the (peer id, address, last cookie) of each configured rendezvous server.
    pub fn servers(&self) -> impl Iterator<Item = (PeerId, Multiaddr, Option<Cookie>)> + '_ {
        self.servers
            .iter()
            .map(|(peer_id, state)| (*peer_id, state.address.clone(), state.cookie.clone()))
    }
}
