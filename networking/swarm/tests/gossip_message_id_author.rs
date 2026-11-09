//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Gossip message ids are scoped to the message's author.
//!
//! Receivers may judge a gossiped message by who authored it, so a copy of some bytes published by one peer must not
//! occupy the id of the same bytes published by another. gossipsub records a message id when the message arrives,
//! before the application reports a verdict, and keeps it after a `Reject`; with author-scoped ids that record only
//! ever deduplicates copies from the same author.

use std::{collections::HashSet, time::Duration};

use libp2p::{
    Multiaddr,
    PeerId,
    futures::{FutureExt, StreamExt, future::select_all},
    gossipsub::{self, IdentTopic, MessageAcceptance, PublishError},
    swarm::SwarmEvent,
};
use tari_swarm::{
    TariNodeBehaviourEvent,
    TariSwarm,
    config::Config,
    create_swarm,
    identity::Keypair,
    messaging::BytesCodec,
};

const TOPIC: &str = "consensus";
const TIMEOUT: Duration = Duration::from_secs(10);

struct Node {
    swarm: TariSwarm<BytesCodec>,
    /// Authors whose messages this node rejects; messages from anyone else are accepted.
    reject_from: HashSet<PeerId>,
    received: Vec<(PeerId, Vec<u8>)>,
    listen_addr: Option<Multiaddr>,
}

impl Node {
    fn new() -> Self {
        let config = Config {
            enable_mdns: false,
            gossip_sub_scored_topics: vec![TOPIC.to_string()],
            ..Default::default()
        };
        let mut swarm =
            create_swarm::<BytesCodec>(Keypair::generate_ed25519(), HashSet::new(), config).expect("create_swarm");
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&IdentTopic::new(TOPIC))
            .expect("subscribe");
        Self {
            swarm,
            reject_from: HashSet::new(),
            received: Vec::new(),
            listen_addr: None,
        }
    }

    fn peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    fn publish(&mut self, payload: &[u8]) -> Result<gossipsub::MessageId, PublishError> {
        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(IdentTopic::new(TOPIC).hash(), payload.to_vec())
    }

    fn sees_subscribed(&self, peer: &PeerId) -> bool {
        let topic = IdentTopic::new(TOPIC).hash();
        self.swarm
            .behaviour()
            .gossipsub
            .all_peers()
            .any(|(p, topics)| p == peer && topics.contains(&&topic))
    }

    fn has_received(&self, author: &PeerId, payload: &[u8]) -> bool {
        self.received.iter().any(|(a, data)| a == author && data == payload)
    }

    fn handle(&mut self, event: SwarmEvent<TariNodeBehaviourEvent<BytesCodec>>) {
        if let SwarmEvent::NewListenAddr { address, .. } = event {
            self.listen_addr.get_or_insert(address);
            return;
        }
        let SwarmEvent::Behaviour(TariNodeBehaviourEvent::Gossipsub(gossipsub::Event::Message {
            message_id,
            message,
            propagation_source,
        })) = event
        else {
            return;
        };
        let author = message.source.expect("strict validation requires an author");
        let acceptance = if self.reject_from.contains(&author) {
            MessageAcceptance::Reject
        } else {
            MessageAcceptance::Accept
        };
        self.swarm.behaviour_mut().gossipsub.report_message_validation_result(
            &message_id,
            &propagation_source,
            acceptance,
        );
        self.received.push((author, message.data));
    }
}

/// Drives every node until `done` holds, or returns false once `timeout` elapses.
async fn drive_until<F>(nodes: &mut [&mut Node], timeout: Duration, mut done: F) -> bool
where F: FnMut(&[&mut Node]) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if done(nodes) {
            return true;
        }
        let (index, event) = {
            let events = nodes
                .iter_mut()
                .enumerate()
                .map(|(i, node)| Box::pin(node.swarm.select_next_some().map(move |event| (i, event))));
            tokio::select! {
                ((index, event), _, _) = select_all(events) => (index, event),
                _ = tokio::time::sleep_until(deadline) => return done(nodes),
            }
        };
        nodes[index].handle(event);
    }
}

async fn listen(nodes: &mut [&mut Node], index: usize) -> Multiaddr {
    nodes[index]
        .swarm
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .expect("listen");
    assert!(
        drive_until(nodes, TIMEOUT, |n| n[index].listen_addr.is_some()).await,
        "node {index} did not start listening"
    );
    nodes[index].listen_addr.clone().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_copy_from_another_author_does_not_suppress_identical_bytes() {
    let mut attacker = Node::new();
    let mut honest = Node::new();
    let mut receiver = Node::new();
    let attacker_id = attacker.peer_id();
    let honest_id = honest.peer_id();
    let receiver_id = receiver.peer_id();
    receiver.reject_from.insert(attacker_id);

    let nodes = &mut [&mut attacker, &mut honest, &mut receiver];
    let addr = listen(nodes, 2).await;
    nodes[0].swarm.dial(addr.clone()).expect("dial");
    nodes[1].swarm.dial(addr).expect("dial");
    assert!(
        drive_until(nodes, TIMEOUT, |n| n[0].sees_subscribed(&receiver_id) &&
            n[1].sees_subscribed(&receiver_id))
        .await,
        "publishers did not see the receiver's subscription"
    );

    let payload = b"identical notification bytes".to_vec();
    nodes[0].publish(&payload).expect("attacker publish");
    assert!(
        drive_until(nodes, TIMEOUT, |n| n[2].has_received(&attacker_id, &payload)).await,
        "receiver did not receive the attacker's copy"
    );

    nodes[1]
        .publish(&payload)
        .expect("honest peer never saw the rejected copy, so it can publish");
    assert!(
        drive_until(nodes, TIMEOUT, |n| n[2].has_received(&honest_id, &payload)).await,
        "the honest author's copy was dropped as a duplicate of the rejected copy from another author"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn author_can_publish_bytes_already_received_from_another_author() {
    let mut other = Node::new();
    let mut author = Node::new();
    let other_id = other.peer_id();
    let author_id = author.peer_id();

    let nodes = &mut [&mut other, &mut author];
    let addr = listen(nodes, 1).await;
    nodes[0].swarm.dial(addr).expect("dial");
    assert!(
        drive_until(nodes, TIMEOUT, |n| n[0].sees_subscribed(&author_id) &&
            n[1].sees_subscribed(&other_id))
        .await,
        "peers did not see each other's subscription"
    );

    let payload = b"identical notification bytes".to_vec();
    nodes[0].publish(&payload).expect("first publish");
    assert!(
        drive_until(nodes, TIMEOUT, |n| n[1].has_received(&other_id, &payload)).await,
        "author did not receive the other peer's copy"
    );

    nodes[1]
        .publish(&payload)
        .expect("a copy from another author must not make the author's own publish a duplicate");
    assert!(
        drive_until(nodes, TIMEOUT, |n| n[0].has_received(&author_id, &payload)).await,
        "the author's copy was not delivered"
    );

    assert!(
        matches!(nodes[1].publish(&payload), Err(PublishError::Duplicate)),
        "the same author's identical bytes must still deduplicate"
    );
}
