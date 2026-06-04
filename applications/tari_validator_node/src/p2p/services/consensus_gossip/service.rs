//  Copyright 2024. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use libp2p::{PeerId, gossipsub};
use log::*;
use tari_consensus::hotstuff::HotstuffEvent;
use tari_epoch_manager::EpochManagerEvent;
use tari_networking::{NetworkingHandle, NetworkingService};
use tari_ootle_p2p::{TariMessagingSpec, proto};
use tari_swarm::messaging::{Codec, prost::ProstCodec};
use tokio::sync::{broadcast, mpsc};

use super::ConsensusGossipError;

const LOG_TARGET: &str = "tari::validator_node::consensus_gossip::service";

/// All consensus gossip is published on a single network-wide topic. Using one topic (rather than a topic per shard
/// group) keeps the gossipsub mesh stable across epoch boundaries, since validators never need to unsubscribe and
/// resubscribe when they are shuffled into a different shard group.
pub const TOPIC_PREFIX: &str = "consensus";

#[derive(Debug)]
pub(super) struct ConsensusGossipService {
    epoch_manager_events: broadcast::Receiver<EpochManagerEvent>,
    consensus_events: broadcast::Receiver<HotstuffEvent>,
    is_subscribed: bool,
    networking: NetworkingHandle<TariMessagingSpec>,
    codec: ProstCodec<proto::consensus::HotStuffMessage>,
    rx_gossip: mpsc::UnboundedReceiver<(PeerId, gossipsub::Message)>,
    tx_consensus_gossip: mpsc::Sender<(PeerId, proto::consensus::HotStuffMessage)>,
}

impl ConsensusGossipService {
    pub fn new(
        epoch_manager_events: broadcast::Receiver<EpochManagerEvent>,
        consensus_events: broadcast::Receiver<HotstuffEvent>,
        networking: NetworkingHandle<TariMessagingSpec>,
        rx_gossip: mpsc::UnboundedReceiver<(PeerId, gossipsub::Message)>,
        tx_consensus_gossip: mpsc::Sender<(PeerId, proto::consensus::HotStuffMessage)>,
    ) -> Self {
        Self {
            epoch_manager_events,
            consensus_events,
            is_subscribed: false,
            networking,
            codec: ProstCodec::default(),
            rx_gossip,
            tx_consensus_gossip,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut initial_subscription_complete = false;
        loop {
            tokio::select! {
                Ok(HotstuffEvent::EpochChanged{ registered_shard_group, .. }) = self.consensus_events.recv() => {
                    if registered_shard_group.is_some() {
                        self.subscribe().await?;
                    } else {
                        self.unsubscribe().await?;
                    }
                },
                Some(msg) = self.rx_gossip.recv() => {
                    if let Err(err) = self.handle_incoming_gossip_message(msg).await {
                        warn!(target: LOG_TARGET, "Consensus gossip service error: {}", err);
                    }
                },
                Ok(EpochManagerEvent::EpochChanged{ registered_shard_group, .. }) = self.epoch_manager_events.recv() => {
                    if !initial_subscription_complete && registered_shard_group.is_some() {
                        self.subscribe().await?;
                        initial_subscription_complete = true;
                    }
                },
                else => {
                    info!(target: LOG_TARGET, "Consensus gossip service shutting down");
                    break;
                }
            }
        }

        self.unsubscribe().await?;

        Ok(())
    }

    async fn handle_incoming_gossip_message(
        &mut self,
        msg: (PeerId, gossipsub::Message),
    ) -> Result<(), ConsensusGossipError> {
        let (from, msg) = msg;

        let (_, msg) = self
            .codec
            .decode_from(&mut msg.data.as_slice())
            .await
            .map_err(|e| ConsensusGossipError::InvalidMessage(e.into()))?;

        self.tx_consensus_gossip
            .send((from, msg))
            .await
            .map_err(|e| ConsensusGossipError::InvalidMessage(e.into()))?;

        Ok(())
    }

    async fn subscribe(&mut self) -> Result<(), ConsensusGossipError> {
        if self.is_subscribed {
            return Ok(());
        }

        info!(target: LOG_TARGET, "🌬️ Consensus gossip service subscribing to {}", topic());
        self.networking.subscribe_topic(topic()).await?;
        self.is_subscribed = true;

        Ok(())
    }

    async fn unsubscribe(&mut self) -> Result<(), ConsensusGossipError> {
        if self.is_subscribed {
            self.networking.unsubscribe_topic(topic()).await?;
            self.is_subscribed = false;
        }

        Ok(())
    }
}

pub(super) fn topic() -> String {
    TOPIC_PREFIX.to_string()
}
