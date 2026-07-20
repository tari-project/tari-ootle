//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use async_trait::async_trait;
use libp2p::gossipsub;
use tokio::sync::oneshot;

mod worker;

mod error;
pub use error::NetworkingError;

mod builder;
mod config;
mod connection;
mod event;
mod global_ip;
mod handle;
mod message;
mod message_mode;
mod notify;
mod peer;
mod relay_state;
mod rendezvous_state;

pub use builder::Builder;
pub use config::*;
pub use connection::*;
pub use handle::*;
pub use message::*;
pub use message_mode::*;
pub use tari_swarm::{
    config::{Config as SwarmConfig, LimitPerInterval, RelayCircuitLimits, RelayReservationLimits},
    identity::PeerId,
    is_supported_multiaddr,
    swarm::{DialError, dial_opts::DialOpts},
};

#[async_trait]
pub trait NetworkingService<TMsg: MessageSpec> {
    fn local_peer_id(&self) -> &PeerId;

    async fn dial_peer<T: Into<DialOpts> + Send + 'static>(
        &mut self,
        dial_opts: T,
    ) -> Result<Waiter<()>, NetworkingError>;

    async fn get_connected_peers(&mut self) -> Result<Vec<PeerId>, NetworkingError>;

    async fn send_message(&mut self, peer: PeerId, message: TMsg::Message) -> Result<(), NetworkingError>;

    /// Sends a message to the specified destination.
    /// Returns the number of messages that were successfully enqueued for sending.
    async fn send_multicast<D: Into<MulticastDestination> + Send + 'static>(
        &mut self,
        destination: D,
        message: TMsg::Message,
    ) -> Result<usize, NetworkingError>;

    async fn publish_gossip<TTopic: Into<String> + Send>(
        &mut self,
        topic: TTopic,
        message: Vec<u8>,
    ) -> Result<(), NetworkingError>;

    /// Reports the outcome of validating an inbound gossip message.
    ///
    /// gossipsub runs in `validate_messages` mode: a message is propagated to the rest of the mesh
    /// only once `Accept` is reported for it, so every message a consumer receives must be reported
    /// exactly once or that topic stops propagating. `Reject` withholds it and counts against the
    /// propagating peer's score; `Ignore` withholds it without penalty, for messages that are
    /// well-formed but uninteresting to us (duplicates, wrong epoch).
    async fn report_gossip_validation(
        &mut self,
        message_id: gossipsub::MessageId,
        propagation_source: PeerId,
        acceptance: gossipsub::MessageAcceptance,
    ) -> Result<(), NetworkingError>;

    async fn subscribe_topic<T: Into<String> + Send>(&mut self, topic: T) -> Result<(), NetworkingError> {
        self.subscribe_topic_with_explicit_peers(topic, Vec::new()).await
    }
    async fn subscribe_topic_with_explicit_peers<T: Into<String> + Send>(
        &mut self,
        topic: T,
        explicit_topic_peers: Vec<PeerId>,
    ) -> Result<(), NetworkingError>;
    async fn unsubscribe_topic<T: Into<String> + Send>(&mut self, topic: T) -> Result<(), NetworkingError>;

    async fn set_want_peers<I: IntoIterator<Item = PeerId> + Send>(&self, want_peers: I)
    -> Result<(), NetworkingError>;
}

pub struct Waiter<T> {
    rx: oneshot::Receiver<Result<T, NetworkingError>>,
}

impl<T> From<oneshot::Receiver<Result<T, NetworkingError>>> for Waiter<T> {
    fn from(rx: oneshot::Receiver<Result<T, NetworkingError>>) -> Self {
        Self { rx }
    }
}

impl<T> Future for Waiter<T> {
    type Output = Result<T, NetworkingError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().rx).poll(cx)?
    }
}
