//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Identify must not turn the listen addresses a peer reports into dial candidates. Those addresses include loopback
//! and container-bridge addresses that name a different host from the dialer's vantage point, so the networking
//! worker decides which of them to record. This test pins that identify itself records none of them: neither in its
//! own cache nor, through `NewExternalAddrOfPeer`, in the peer store.

use std::{collections::HashSet, time::Duration};

use libp2p::{
    Multiaddr,
    core::Endpoint,
    futures::StreamExt,
    identify,
    swarm::{ConnectionId, NetworkBehaviour, SwarmEvent, dial_opts::DialOpts},
};
use libp2p_peer_store::Store;
use tari_swarm::{
    TariNodeBehaviourEvent,
    config::Config,
    create_swarm,
    identity::Keypair,
    messaging::prost::ProstCodec,
};
use tokio::time::{Instant, error::Elapsed, timeout_at};

type TestSwarm = libp2p::Swarm<tari_swarm::TariNodeBehaviour<ProstCodec<Vec<u8>>>>;

fn new_swarm() -> TestSwarm {
    let config = Config {
        enable_mdns: false,
        ..Default::default()
    };
    create_swarm::<ProstCodec<Vec<u8>>>(Keypair::generate_ed25519(), HashSet::new(), config).expect("create_swarm")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identify_records_no_reported_listen_addresses() {
    let reported_addr: Multiaddr = "/ip4/172.17.0.1/tcp/50123".parse().unwrap();

    let mut listener = new_swarm();
    let listener_id = *listener.local_peer_id();
    listener.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
    let listen_addr = loop {
        if let SwarmEvent::NewListenAddr { address, .. } = listener.select_next_some().await {
            break address;
        }
    };
    // Identify reports external addresses alongside listen addresses, so this stands in for a bridge address the
    // listener reports.
    listener.add_external_address(reported_addr.clone());

    let mut dialer = new_swarm();
    dialer
        .dial(DialOpts::peer_id(listener_id).addresses(vec![listen_addr]).build())
        .unwrap();

    // Identify emits any `NewExternalAddrOfPeer` after the `Received` event, so once identified the swarms keep running
    // until `settle_until` for the peer store to see them.
    let identify_deadline = Instant::now() + Duration::from_secs(10);
    let mut settle_until = None;
    loop {
        let event = match timeout_at(settle_until.unwrap_or(identify_deadline), async {
            tokio::select! {
                ev = dialer.select_next_some() => Some(ev),
                _ = listener.select_next_some() => None,
            }
        })
        .await
        {
            Ok(event) => event,
            Err(Elapsed { .. }) if settle_until.is_some() => break,
            Err(Elapsed { .. }) => panic!("dialer did not receive an identify from the listener in time"),
        };

        if let Some(SwarmEvent::Behaviour(TariNodeBehaviourEvent::Identify(identify::Event::Received {
            peer_id,
            info,
            ..
        }))) = event &&
            peer_id == listener_id
        {
            assert!(
                info.listen_addrs.contains(&reported_addr),
                "listener must report {reported_addr} for this test to exercise anything"
            );
            settle_until.get_or_insert_with(|| Instant::now() + Duration::from_millis(500));
        }
    }

    let stored = dialer
        .behaviour()
        .peer_store
        .store()
        .addresses_of_peer(&listener_id)
        .map(|addrs| addrs.cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(
        !stored.contains(&reported_addr),
        "identify put a reported listen address in the peer store: {stored:?}"
    );

    let offered = dialer
        .behaviour_mut()
        .identify
        .handle_pending_outbound_connection(ConnectionId::new_unchecked(0), Some(listener_id), &[], Endpoint::Dialer)
        .unwrap();
    assert!(
        offered.is_empty(),
        "identify offered cached listen addresses for a dial: {offered:?}"
    );
}
