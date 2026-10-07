//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashMap,
    convert::Infallible,
    fmt,
    net::IpAddr,
    task::{Context, Poll},
};

use libp2p::{
    Multiaddr,
    PeerId,
    core::{Endpoint, multiaddr::Protocol, transport::PortUse},
    swarm::{
        ConnectionClosed,
        ConnectionDenied,
        ConnectionId,
        FromSwarm,
        ListenFailure,
        NetworkBehaviour,
        THandler,
        THandlerInEvent,
        THandlerOutEvent,
        ToSwarm,
        dummy,
    },
};

/// Caps the inbound connections, pending or established, from a single IP address.
///
/// A PeerId costs nothing to generate, so per-peer limits alone do not bound what one host can open.
/// Relayed connections are not counted, since their address names the relay rather than the remote
/// host, and neither are loopback connections, which only a process on this machine can open.
pub struct Behaviour {
    max_per_ip: u32,
    per_ip: HashMap<IpAddr, u32>,
    connections: HashMap<ConnectionId, IpAddr>,
}

impl Behaviour {
    pub fn new(max_per_ip: u32) -> Self {
        Self {
            max_per_ip,
            per_ip: HashMap::new(),
            connections: HashMap::new(),
        }
    }
}

fn remote_ip(addr: &Multiaddr) -> Option<IpAddr> {
    if addr.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
        return None;
    }
    addr.iter()
        .find_map(|p| match p {
            Protocol::Ip4(ip) => Some(IpAddr::V4(ip)),
            Protocol::Ip6(ip) => Some(IpAddr::V6(ip)),
            _ => None,
        })
        .filter(|ip| !ip.to_canonical().is_loopback())
}

#[derive(Debug, Clone, Copy)]
pub struct Exceeded {
    pub ip: IpAddr,
    pub limit: u32,
}

impl fmt::Display for Exceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} already has {} inbound connections", self.ip, self.limit)
    }
}

impl std::error::Error for Exceeded {}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Infallible;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        let Some(ip) = remote_ip(remote_addr) else {
            return Ok(());
        };
        let count = self.per_ip.entry(ip).or_default();
        if *count >= self.max_per_ip {
            return Err(ConnectionDenied::new(Exceeded {
                ip,
                limit: self.max_per_ip,
            }));
        }
        *count += 1;
        self.connections.insert(connection_id, ip);
        Ok(())
    }

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        let connection_id = match event {
            FromSwarm::ConnectionClosed(ConnectionClosed { connection_id, .. }) |
            FromSwarm::ListenFailure(ListenFailure { connection_id, .. }) => connection_id,
            _ => return,
        };
        if let Some(ip) = self.connections.remove(&connection_id) &&
            let Some(count) = self.per_ip.get_mut(&ip)
        {
            *count -= 1;
            if *count == 0 {
                self.per_ip.remove(&ip);
            }
        }
    }

    fn on_connection_handler_event(&mut self, _: PeerId, _: ConnectionId, event: THandlerOutEvent<Self>) {
        match event {}
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relayed_and_loopback_addresses_are_not_counted() {
        let direct: Multiaddr = "/ip4/1.2.3.4/tcp/1".parse().unwrap();
        let relayed: Multiaddr = "/ip4/1.2.3.4/tcp/1/p2p-circuit".parse().unwrap();
        let loopback: Multiaddr = "/ip4/127.0.0.1/tcp/1".parse().unwrap();
        assert_eq!(remote_ip(&direct), Some("1.2.3.4".parse().unwrap()));
        assert_eq!(remote_ip(&relayed), None);
        let mapped_loopback: Multiaddr = "/ip6/::ffff:127.0.0.1/tcp/1".parse().unwrap();
        assert_eq!(remote_ip(&loopback), None);
        assert_eq!(remote_ip(&mapped_loopback), None);
    }

    #[test]
    fn concurrent_dials_from_one_ip_count_before_any_is_established() {
        let mut limits = Behaviour::new(2);
        let local: Multiaddr = "/ip4/0.0.0.0/tcp/1".parse().unwrap();
        let remote: Multiaddr = "/ip4/1.2.3.4/tcp/1".parse().unwrap();
        let other: Multiaddr = "/ip4/5.6.7.8/tcp/1".parse().unwrap();

        for id in 0..2 {
            limits
                .handle_pending_inbound_connection(ConnectionId::new_unchecked(id), &local, &remote)
                .unwrap();
        }
        assert!(
            limits
                .handle_pending_inbound_connection(ConnectionId::new_unchecked(2), &local, &remote)
                .is_err()
        );
        limits
            .handle_pending_inbound_connection(ConnectionId::new_unchecked(3), &local, &other)
            .unwrap();
    }
}
