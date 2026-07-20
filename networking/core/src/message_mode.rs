//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use libp2p::{PeerId, gossipsub};
use tokio::sync::mpsc;

use crate::message::MessageSpec;

pub const TOPIC_DELIMITER: &str = "-";

#[derive(Debug, thiserror::Error)]
pub enum GossipSendError {
    #[error("Invalid token topic: {0}")]
    InvalidToken(String),
    #[error("Inbound gossip channel closed")]
    InboundGossipChannelClosed,
}

impl From<mpsc::error::SendError<GossipMessage>> for GossipSendError {
    fn from(_: mpsc::error::SendError<GossipMessage>) -> Self {
        Self::InboundGossipChannelClosed
    }
}

/// An inbound gossipsub message, carrying the identifiers needed to report its validation verdict.
///
/// gossipsub runs in `validate_messages` mode, so a message is held and propagated to nobody until a
/// verdict is reported for it. A consumer MUST call
/// [`crate::NetworkingService::report_gossip_validation`] for every message it receives, or that
/// topic stops propagating across the network.
#[derive(Debug)]
pub struct GossipMessage {
    /// The peer that authored the message.
    pub source: PeerId,
    /// The peer we received it from, which may differ from `source`.
    pub propagation_source: PeerId,
    pub message_id: gossipsub::MessageId,
    pub message: gossipsub::Message,
}

impl GossipMessage {
    /// The pair identifying this message to [`crate::NetworkingService::report_gossip_validation`].
    pub fn validation_key(&self) -> (gossipsub::MessageId, PeerId) {
        (self.message_id.clone(), self.propagation_source)
    }
}

pub enum MessagingMode<TMsg: MessageSpec> {
    Enabled {
        tx_messages: mpsc::UnboundedSender<(PeerId, TMsg::Message)>,
        tx_gossip_messages_by_topic: HashMap<String, mpsc::UnboundedSender<GossipMessage>>,
    },
    Disabled,
}

impl<TMsg: MessageSpec> MessagingMode<TMsg> {
    pub fn is_enabled(&self) -> bool {
        matches!(self, MessagingMode::Enabled { .. })
    }
}

impl<TMsg: MessageSpec> MessagingMode<TMsg> {
    pub fn send_message(
        &self,
        peer_id: PeerId,
        msg: TMsg::Message,
    ) -> Result<(), mpsc::error::SendError<(PeerId, TMsg::Message)>> {
        if let MessagingMode::Enabled { tx_messages, .. } = self {
            tx_messages.send((peer_id, msg))?;
        }
        Ok(())
    }

    pub fn send_gossip_message(&self, msg: GossipMessage) -> Result<(), GossipSendError> {
        if let MessagingMode::Enabled {
            tx_gossip_messages_by_topic,
            ..
        } = self
        {
            // Topics may be a bare prefix (single global topic, e.g. "consensus") or a prefixed topic
            // (e.g. "transactions-0-15"). Route on the prefix before the first delimiter, falling back to the
            // whole topic when there is no delimiter.
            let topic = msg.message.topic.as_str();
            let prefix = topic.split_once(TOPIC_DELIMITER).map_or(topic, |(prefix, _)| prefix);
            let tx_gossip_messages = tx_gossip_messages_by_topic
                .get(prefix)
                .ok_or_else(|| GossipSendError::InvalidToken(topic.to_string()))?;
            tx_gossip_messages.send(msg)?;
        }
        Ok(())
    }
}
