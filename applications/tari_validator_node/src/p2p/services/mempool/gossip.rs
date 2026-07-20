//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use libp2p::{PeerId, gossipsub};
use log::*;
use tari_networking::{GossipMessage, NetworkingHandle, NetworkingService};
use tari_ootle_p2p::{NewTransactionMessage, PeerAddress, TariMessage, TariMessagingSpec, proto};
use tari_swarm::messaging::{Codec, prost::ProstCodec};
use tokio::sync::mpsc;

use crate::p2p::services::mempool::MempoolError;

const LOG_TARGET: &str = "tari::validator_node::mempool::gossip";

/// All transactions are gossiped on a single network-wide topic. Using one topic (rather than a topic per shard group)
/// keeps the gossipsub mesh stable across epoch boundaries, since validators never need to unsubscribe and resubscribe
/// when they are shuffled into a different shard group.
pub const TOPIC_PREFIX: &str = "transactions";

#[derive(Debug)]
pub struct MempoolGossipCodec {
    codec: ProstCodec<proto::network::TariMessage>,
}

impl MempoolGossipCodec {
    pub fn new() -> Self {
        Self {
            codec: ProstCodec::default(),
        }
    }

    pub async fn encode(&self, message: TariMessage) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(1024);
        let message = proto::network::TariMessage::from(&message);
        self.codec.encode_to(&mut buf, message).await?;
        Ok(buf)
    }

    pub async fn decode(&self, message: gossipsub::Message) -> std::io::Result<(usize, TariMessage)> {
        let (length, message) = self.codec.decode_from(&mut message.data.as_slice()).await?;
        let message = TariMessage::try_from(message).map_err(std::io::Error::other)?;

        Ok((length, message))
    }
}

#[derive(Debug)]
pub(super) struct MempoolGossip {
    is_subscribed: bool,
    networking: NetworkingHandle<TariMessagingSpec>,
    rx_gossip: mpsc::UnboundedReceiver<GossipMessage>,
    codec: MempoolGossipCodec,
}

impl MempoolGossip {
    pub fn new(
        networking: NetworkingHandle<TariMessagingSpec>,
        rx_gossip: mpsc::UnboundedReceiver<GossipMessage>,
    ) -> Self {
        Self {
            is_subscribed: false,
            networking,
            rx_gossip,
            codec: MempoolGossipCodec::new(),
        }
    }

    pub async fn next_message(&mut self) -> Option<Result<IncomingMessage, MempoolError>> {
        let gossip = self.rx_gossip.recv().await?;
        // Number of transactions still to receive
        let num_pending = self.rx_gossip.len();
        let validation = GossipValidation::new(gossip.validation_key());
        let from = gossip.source;
        match self.codec.decode(gossip.message).await {
            Ok((msg_len, msg)) => Some(Ok(IncomingMessage {
                address: from.into(),
                message: msg,
                num_pending,
                message_size: msg_len,
                validation,
            })),
            Err(e) => {
                // Undecodable: withhold from the mesh and count it against the peer that sent it.
                self.report(validation, gossipsub::MessageAcceptance::Reject).await;
                Some(Err(MempoolError::InvalidMessage(e.into())))
            },
        }
    }

    /// Reports an inbound message's validation verdict. gossipsub withholds the message from the
    /// rest of the mesh until this is called, so every message taken from `next_message` must be
    /// reported exactly once.
    pub async fn report(&mut self, validation: GossipValidation, acceptance: gossipsub::MessageAcceptance) {
        let (message_id, propagation_source) = validation.into_key();
        if let Err(e) = self
            .networking
            .report_gossip_validation(message_id, propagation_source, acceptance)
            .await
        {
            warn!(target: LOG_TARGET, "Failed to report gossip validation result: {e}");
        }
    }

    pub async fn subscribe(&mut self) -> Result<(), MempoolError> {
        if self.is_subscribed {
            return Ok(());
        }

        self.networking.subscribe_topic(topic()).await?;
        self.is_subscribed = true;
        Ok(())
    }

    pub async fn unsubscribe(&mut self) -> Result<(), MempoolError> {
        if self.is_subscribed {
            self.networking.unsubscribe_topic(topic()).await?;
            self.is_subscribed = false;
        }
        Ok(())
    }

    pub fn get_num_incoming_messages(&self) -> usize {
        self.rx_gossip.len()
    }

    /// Gossip a transaction to the rest of the network. Since transactions are published on a single network-wide
    /// topic, gossipsub delivers the transaction to every validator with one publish; replicas in the involved shard
    /// groups pick it up directly and no per-shard-group forwarding is required.
    pub async fn forward(&mut self, msg: NewTransactionMessage) -> Result<(), MempoolError> {
        debug!(target: LOG_TARGET, "forward transaction on topic: {}", topic());
        let msg = self
            .codec
            .encode(msg.into())
            .await
            .map_err(|e| MempoolError::InvalidMessage(e.into()))?;
        self.networking.publish_gossip(topic(), msg).await?;

        Ok(())
    }
}

fn topic() -> String {
    TOPIC_PREFIX.to_string()
}

pub struct IncomingMessage {
    pub address: PeerAddress,
    pub message: TariMessage,
    pub num_pending: usize,
    pub message_size: usize,
    /// Identifies this message when reporting its validation verdict. Must be passed to
    /// [`MempoolGossip::report`] exactly once, or the message is never propagated onward.
    pub validation: GossipValidation,
}

/// Handle identifying one inbound gossip message for the purpose of reporting its validation
/// verdict. Deliberately not `Clone`: a verdict is reported once per message.
#[derive(Debug)]
pub struct GossipValidation {
    key: (gossipsub::MessageId, PeerId),
}

impl GossipValidation {
    fn new(key: (gossipsub::MessageId, PeerId)) -> Self {
        Self { key }
    }

    fn into_key(self) -> (gossipsub::MessageId, PeerId) {
        self.key
    }
}
