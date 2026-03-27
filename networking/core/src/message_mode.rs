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

impl From<mpsc::error::SendError<(PeerId, gossipsub::Message)>> for GossipSendError {
    fn from(_: mpsc::error::SendError<(PeerId, gossipsub::Message)>) -> Self {
        Self::InboundGossipChannelClosed
    }
}

pub enum MessagingMode<TMsg: MessageSpec> {
    Enabled {
        tx_messages: mpsc::UnboundedSender<(PeerId, TMsg::Message)>,
        tx_gossip_messages_by_topic: HashMap<String, mpsc::UnboundedSender<(PeerId, gossipsub::Message)>>,
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

    pub fn send_gossip_message(&self, peer_id: PeerId, msg: gossipsub::Message) -> Result<(), GossipSendError> {
        if let MessagingMode::Enabled {
            tx_gossip_messages_by_topic,
            ..
        } = self
        {
            let (prefix, _) = msg
                .topic
                .as_str()
                .split_once(TOPIC_DELIMITER)
                .ok_or_else(|| GossipSendError::InvalidToken(msg.topic.clone().into_string()))?;
            let tx_gossip_messages = tx_gossip_messages_by_topic
                .get(prefix)
                .ok_or_else(|| GossipSendError::InvalidToken(msg.topic.clone().into_string()))?;
            tx_gossip_messages.send((peer_id, msg))?;
        }
        Ok(())
    }
}
