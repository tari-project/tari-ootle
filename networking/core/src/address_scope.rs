//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::net::IpAddr;

use libp2p::{Multiaddr, multiaddr::Protocol};

use crate::global_ip::GlobalIp;

/// How far from the local host an address can be reached, ordered from widest to narrowest.
///
/// A peer's listen address is meaningful only from a vantage point that shares its scope: `127.0.0.1` or a container
/// bridge address names a different host for every dialer outside the peer's host or network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AddressScope {
    Global,
    Private,
    Loopback,
}

impl AddressScope {
    /// DNS names and bare relay circuits are resolved or routed beyond the local network, so they are classed as
    /// global.
    pub fn of(addr: &Multiaddr) -> Self {
        let ip = match addr.iter().next() {
            Some(Protocol::Ip4(ip)) => IpAddr::V4(ip),
            Some(Protocol::Ip6(ip)) => IpAddr::V6(ip),
            _ => return AddressScope::Global,
        };
        if ip.is_loopback() || ip.is_unspecified() {
            return AddressScope::Loopback;
        }
        let is_global = match ip {
            IpAddr::V4(ip) => ip.is_global_ip(),
            IpAddr::V6(ip) => ip.is_global_ip(),
        };
        if is_global {
            AddressScope::Global
        } else {
            AddressScope::Private
        }
    }
}

/// Returns the listen addresses a peer reports that are dialable given the address the peer is connected on.
///
/// A peer connected over a global address is on another host and network, so only its global addresses can reach
/// it. One connected over a private address shares a network, so its private addresses are dialable too, but its
/// loopback addresses still name the local host. Only a peer connected over loopback shares the host.
pub(crate) fn dialable_listen_addresses<'a, I>(
    listen_addrs: I,
    connected_on: &Multiaddr,
) -> impl Iterator<Item = &'a Multiaddr> + 'a
where
    I: IntoIterator<Item = &'a Multiaddr>,
    I::IntoIter: 'a,
{
    let connection_scope = AddressScope::of(connected_on);
    listen_addrs
        .into_iter()
        .filter(move |addr| AddressScope::of(addr) <= connection_scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> Multiaddr {
        s.parse().unwrap()
    }

    fn dialable(listen_addrs: &[Multiaddr], connected_on: &str) -> Vec<Multiaddr> {
        dialable_listen_addresses(listen_addrs, &addr(connected_on))
            .cloned()
            .collect()
    }

    #[test]
    fn it_classifies_addresses() {
        assert_eq!(AddressScope::of(&addr("/ip4/127.0.0.1/tcp/1")), AddressScope::Loopback);
        assert_eq!(AddressScope::of(&addr("/ip4/0.0.0.0/tcp/1")), AddressScope::Loopback);
        assert_eq!(AddressScope::of(&addr("/ip6/::1/tcp/1")), AddressScope::Loopback);
        assert_eq!(AddressScope::of(&addr("/ip4/172.17.0.1/tcp/1")), AddressScope::Private);
        assert_eq!(AddressScope::of(&addr("/ip4/192.168.1.5/tcp/1")), AddressScope::Private);
        assert_eq!(AddressScope::of(&addr("/ip6/fe80::1/tcp/1")), AddressScope::Private);
        assert_eq!(AddressScope::of(&addr("/ip4/51.68.1.2/tcp/1")), AddressScope::Global);
        assert_eq!(AddressScope::of(&addr("/dns4/example.com/tcp/1")), AddressScope::Global);
    }

    #[test]
    fn it_keeps_only_global_addresses_for_a_globally_connected_peer() {
        let listen_addrs = [
            addr("/ip4/127.0.0.1/tcp/50123"),
            addr("/ip4/172.17.0.1/tcp/50123"),
            addr("/ip4/51.68.1.2/tcp/50123"),
            addr("/ip4/51.68.1.2/udp/50123/quic-v1"),
            addr("/dns4/vn.example.com/tcp/50123"),
        ];
        assert_eq!(dialable(&listen_addrs, "/ip4/51.68.1.2/tcp/50123"), vec![
            addr("/ip4/51.68.1.2/tcp/50123"),
            addr("/ip4/51.68.1.2/udp/50123/quic-v1"),
            addr("/dns4/vn.example.com/tcp/50123"),
        ]);
    }

    #[test]
    fn it_drops_loopback_addresses_for_a_peer_on_the_local_network() {
        let listen_addrs = [
            addr("/ip4/127.0.0.1/tcp/50123"),
            addr("/ip4/192.168.1.5/tcp/50123"),
            addr("/ip4/51.68.1.2/tcp/50123"),
        ];
        assert_eq!(dialable(&listen_addrs, "/ip4/192.168.1.5/tcp/40000"), vec![
            addr("/ip4/192.168.1.5/tcp/50123"),
            addr("/ip4/51.68.1.2/tcp/50123"),
        ]);
    }

    #[test]
    fn it_keeps_every_address_for_a_peer_on_the_same_host() {
        let listen_addrs = [
            addr("/ip4/127.0.0.1/tcp/50123"),
            addr("/ip4/172.17.0.1/tcp/50123"),
            addr("/ip4/51.68.1.2/tcp/50123"),
        ];
        assert_eq!(
            dialable(&listen_addrs, "/ip4/127.0.0.1/tcp/40000"),
            listen_addrs.to_vec()
        );
    }
}
