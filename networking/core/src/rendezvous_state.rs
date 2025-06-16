//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use libp2p::Multiaddr;
use tari_swarm::rendezvous::Cookie;

use crate::PeerId;

pub struct RendezvousState {
    rendezvous_server: Option<(PeerId, RvPeerState)>,
}

#[derive(Debug, Clone)]
pub struct RvPeerState {
    address: Multiaddr,
    cookie: Option<Cookie>,
}

impl RendezvousState {
    pub fn new(server: Option<(PeerId, Multiaddr)>) -> Self {
        Self {
            rendezvous_server: server.map(|(peer_id, addr)| {
                (peer_id, RvPeerState {
                    address: addr,
                    cookie: None,
                })
            }),
        }
    }

    pub fn set_cookie(&mut self, cookie: Cookie) -> bool {
        match self.rendezvous_server.as_mut() {
            Some((_, state_mut)) => {
                state_mut.cookie = Some(cookie);
                true
            },
            None => false,
        }
    }

    pub fn get_peer_and_cookie(&self) -> Option<(PeerId, Option<Cookie>)> {
        self.rendezvous_server
            .as_ref()
            .map(|(peer_id, state)| (*peer_id, state.cookie.clone()))
    }

    pub fn get_peer_and_address(&self) -> Option<(PeerId, Multiaddr)> {
        self.peer_and_address().map(|(peer_id, addr)| (*peer_id, addr.clone()))
    }

    pub fn peer_and_address(&self) -> Option<(&PeerId, &Multiaddr)> {
        self.rendezvous_server
            .as_ref()
            .map(|(peer_id, state)| (peer_id, &state.address))
    }
}
