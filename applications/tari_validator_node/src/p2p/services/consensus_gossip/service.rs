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

use libp2p::{PeerId, gossipsub::MessageAcceptance};
use log::*;
use tari_consensus::{hotstuff::HotstuffEvent, messages::HotstuffMessage};
use tari_epoch_manager::EpochManagerEvent;
use tari_networking::{GossipMessage, NetworkingHandle, NetworkingService};
use tari_ootle_p2p::{
    TariMessagingSpec,
    decode_budget::{CONSENSUS_GOSSIP_MAX_DECODE_ITEMS, decode_with_max_items},
    proto,
};
use tokio::sync::{broadcast, mpsc};

use super::ConsensusGossipError;

const LOG_TARGET: &str = "tari::validator_node::consensus_gossip::service";

/// All consensus gossip is published on a single network-wide topic. Using one topic (rather than a topic per shard
/// group) keeps the gossipsub mesh stable across epoch boundaries, since validators never need to unsubscribe and
/// resubscribe when they are shuffled into a different shard group.
pub const TOPIC_PREFIX: &str = "consensus";

/// Maximum wire body accepted on the `"consensus"` gossip topic, excluding the 4-byte length prefix.
///
/// Gossipsub's global frame limit is sized for full transactions (`max_gossip_message_size`), whereas consensus only
/// broadcasts `ForeignProposalNotification` (`32`-byte `block_id`, `u64` `epoch`, `u32` `shard_group`, and at most
/// `256` `u32` `shard_groups` — under `1.6 KiB` encoded). Capping the frame body here rejects oversized gossip
/// payloads before allocating or invoking `prost`.
const MAX_CONSENSUS_GOSSIP_PAYLOAD_BYTES: usize = 4 * 1024;

#[derive(Debug)]
pub(super) struct ConsensusGossipService {
    epoch_manager_events: broadcast::Receiver<EpochManagerEvent>,
    consensus_events: broadcast::Receiver<HotstuffEvent>,
    is_subscribed: bool,
    networking: NetworkingHandle<TariMessagingSpec>,
    rx_gossip: mpsc::Receiver<GossipMessage>,
    tx_consensus_gossip: mpsc::Sender<(PeerId, HotstuffMessage)>,
}

impl ConsensusGossipService {
    pub fn new(
        epoch_manager_events: broadcast::Receiver<EpochManagerEvent>,
        consensus_events: broadcast::Receiver<HotstuffEvent>,
        networking: NetworkingHandle<TariMessagingSpec>,
        rx_gossip: mpsc::Receiver<GossipMessage>,
        tx_consensus_gossip: mpsc::Sender<(PeerId, HotstuffMessage)>,
    ) -> Self {
        Self {
            epoch_manager_events,
            consensus_events,
            is_subscribed: false,
            networking,
            rx_gossip,
            tx_consensus_gossip,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut initial_subscription_complete = false;
        loop {
            tokio::select! {
                result = self.consensus_events.recv() => {
                    match result {
                        Ok(HotstuffEvent::EpochChanged { registered_shard_group, .. }) => {
                            initial_subscription_complete = true;
                            if registered_shard_group.is_some() {
                                self.subscribe().await?;
                            } else {
                                self.unsubscribe().await?;
                            }
                        },
                        Ok(_) => {},
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(target: LOG_TARGET, "Consensus events receiver lagged by {} messages", n);
                        },
                        Err(broadcast::error::RecvError::Closed) => {
                            info!(
                                target: LOG_TARGET,
                                "Consensus events channel closed; consensus gossip service stopping"
                            );
                            break;
                        },
                    }
                },
                msg = self.rx_gossip.recv() => {
                    let Some(msg) = msg else {
                        info!(
                            target: LOG_TARGET,
                            "Gossip channel closed; consensus gossip service stopping"
                        );
                        break;
                    };
                    if let Err(err) = self.handle_incoming_gossip_message(msg).await {
                        warn!(target: LOG_TARGET, "Consensus gossip service error: {}", err);
                    }
                },
                result = self.epoch_manager_events.recv() => {
                    match result {
                        Ok(EpochManagerEvent::EpochChanged { registered_shard_group, .. }) => {
                            if !initial_subscription_complete && registered_shard_group.is_some() {
                                self.subscribe().await?;
                                initial_subscription_complete = true;
                            }
                        },
                        Ok(EpochManagerEvent::PendingActivation { .. }) => {},
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(target: LOG_TARGET, "Epoch manager events receiver lagged by {} messages", n);
                        },
                        Err(broadcast::error::RecvError::Closed) => {
                            info!(
                                target: LOG_TARGET,
                                "Epoch manager events channel closed; consensus gossip service stopping"
                            );
                            break;
                        },
                    }
                },
            }
        }

        self.unsubscribe().await?;

        Ok(())
    }

    async fn handle_incoming_gossip_message(&mut self, gossip: GossipMessage) -> Result<(), ConsensusGossipError> {
        let (message_id, propagation_source) = gossip.validation_key();
        let from = gossip.source;

        let decoded = decode_gossip_payload(&gossip.message.data);

        // gossipsub withholds the message from the mesh until a verdict is reported. A message that
        // is not a well-formed broadcast is withheld and counted against the peer that sent it;
        // anything else is accepted so it continues to propagate.
        let acceptance = if decoded.is_ok() {
            MessageAcceptance::Accept
        } else {
            MessageAcceptance::Reject
        };
        if let Err(e) = self
            .networking
            .report_gossip_validation(message_id, propagation_source, acceptance)
            .await
        {
            warn!(target: LOG_TARGET, "Failed to report gossip validation result: {e}");
        }

        let msg = decoded.map_err(ConsensusGossipError::InvalidMessage)?;

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

fn decode_gossip_payload(data: &[u8]) -> anyhow::Result<HotstuffMessage> {
    const FRAME_HEADER_LEN: usize = size_of::<u32>();
    let Some((len_bytes, payload)) = data.split_first_chunk::<FRAME_HEADER_LEN>() else {
        return Err(anyhow::anyhow!(
            "Consensus gossip payload is shorter than the {FRAME_HEADER_LEN}-byte frame header ({} byte(s))",
            data.len()
        ));
    };
    let frame_len = u32::from_be_bytes(*len_bytes) as usize;
    if frame_len > MAX_CONSENSUS_GOSSIP_PAYLOAD_BYTES {
        return Err(anyhow::anyhow!(
            "Consensus gossip frame of {frame_len} bytes exceeds maximum {MAX_CONSENSUS_GOSSIP_PAYLOAD_BYTES} bytes"
        ));
    }
    if payload.len() < frame_len {
        return Err(anyhow::anyhow!(
            "Consensus gossip frame truncated: header declares {frame_len} bytes, {} available",
            payload.len()
        ));
    }
    let trailing = payload.len() - frame_len;
    if trailing != 0 {
        return Err(anyhow::anyhow!(
            "Consensus gossip payload has {trailing} trailing byte(s)"
        ));
    }
    let msg = decode_with_max_items::<proto::consensus::HotStuffMessage>(
        &payload[..frame_len],
        CONSENSUS_GOSSIP_MAX_DECODE_ITEMS,
    )?;
    decode_gossiped_message(msg)
}

/// Converts a message received on the consensus topic, refusing any kind consensus never broadcasts before
/// its payload is decoded. Every other kind travels point to point, where the receiver can tell who sent it.
fn decode_gossiped_message(msg: proto::consensus::HotStuffMessage) -> anyhow::Result<HotstuffMessage> {
    match &msg.message {
        Some(proto::consensus::hot_stuff_message::Message::ForeignProposalNotification(_)) => {
            HotstuffMessage::try_from(msg)
        },
        Some(_) => Err(anyhow::anyhow!(
            "Peer gossiped a consensus message that is only sent point to point"
        )),
        None => Err(anyhow::anyhow!("Peer gossiped an empty consensus message")),
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::{ShardGroup, shard::Shard};
    use tari_ootle_p2p::proto::{
        consensus::{
            ForeignProposalNotification,
            HotStuffMessage,
            MissingTransactionsResponse,
            hot_stuff_message::Message,
        },
        transaction::Transaction,
    };
    use tari_swarm::messaging::{Codec, prost::ProstCodec};

    use super::*;

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(size_of::<u32>() + body.len());
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(body);
        frame
    }

    fn len_field(tag: u32, contents: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(contents.len().saturating_add(16));
        prost::encoding::encode_key(tag, prost::encoding::WireType::LengthDelimited, &mut buf);
        prost::encoding::encode_varint(contents.len() as u64, &mut buf);
        buf.extend_from_slice(contents);
        buf
    }

    #[test]
    fn a_foreign_proposal_notification_is_accepted() {
        let msg = HotStuffMessage {
            message: Some(Message::ForeignProposalNotification(ForeignProposalNotification {
                block_id: vec![1; 32],
                epoch: 1,
                shard_groups: vec![],
                shard_group: ShardGroup::new(1u32, 1u32).encode_as_u32(),
            })),
        };
        let decoded = decode_gossiped_message(msg).unwrap();
        assert!(matches!(decoded, HotstuffMessage::ForeignProposalNotification(_)));
    }

    #[test]
    fn a_point_to_point_message_is_refused_before_its_payload_is_decoded() {
        // The transaction bytes do not decode, so an error about the message kind shows they were never tried.
        let msg = HotStuffMessage {
            message: Some(Message::RequestedTransaction(MissingTransactionsResponse {
                request_id: 1,
                epoch: 1,
                block_id: vec![1; 32],
                transactions: vec![Transaction {
                    bor_encoded: vec![0xff; 16],
                }],
            })),
        };
        let err = decode_gossiped_message(msg).unwrap_err();
        assert!(err.to_string().contains("point to point"), "{err}");
    }

    #[tokio::test]
    async fn a_gossip_payload_with_trailing_bytes_is_refused() {
        let codec = ProstCodec::<HotStuffMessage>::default();
        let max_shards = Shard::max().as_u32();
        let msg = HotStuffMessage {
            message: Some(Message::ForeignProposalNotification(ForeignProposalNotification {
                block_id: vec![1; 32],
                epoch: 1,
                shard_groups: (1..=max_shards)
                    .map(|s| ShardGroup::new(s, s).encode_as_u32())
                    .collect(),
                shard_group: ShardGroup::new(1u32, max_shards).encode_as_u32(),
            })),
        };
        let mut buf = Vec::new();
        codec.encode_to(&mut buf, msg).await.unwrap();
        assert!(decode_gossip_payload(&buf).is_ok());

        buf.extend_from_slice(&[0xaa, 0xbb]);
        let err = decode_gossip_payload(&buf).unwrap_err();
        assert!(err.to_string().contains("trailing byte(s)"), "{err}");
    }

    #[test]
    fn an_oversized_or_truncated_gossip_frame_is_refused_before_prost_decodes_it() {
        let oversized = framed(&vec![0u8; MAX_CONSENSUS_GOSSIP_PAYLOAD_BYTES + 1]);
        let err = decode_gossip_payload(&oversized).unwrap_err();
        assert!(err.to_string().contains("exceeds maximum"), "{err}");

        // A 5-byte payload claiming a 4 MiB frame length is rejected without allocating the declared frame.
        let mut bogus_header = (4 * 1024 * 1024u32).to_be_bytes().to_vec();
        bogus_header.push(0x00);
        let err = decode_gossip_payload(&bogus_header).unwrap_err();
        assert!(err.to_string().contains("exceeds maximum"), "{err}");

        let mut truncated = (64u32).to_be_bytes().to_vec();
        truncated.extend_from_slice(&[0x01, 0x02]);
        let err = decode_gossip_payload(&truncated).unwrap_err();
        assert!(err.to_string().contains("truncated"), "{err}");

        let err = decode_gossip_payload(&[0x00, 0x01]).unwrap_err();
        assert!(err.to_string().contains("frame header"), "{err}");
    }

    #[test]
    fn an_over_budget_gossip_frame_within_the_byte_cap_is_refused_before_prost_allocates() {
        // ProposalMessage (tag 2) -> Block (tag 1) -> repeated empty Command (tag 3) fitting inside 4 KiB
        let commands = len_field(3, &[]).repeat(CONSENSUS_GOSSIP_MAX_DECODE_ITEMS + 1);
        let body = len_field(2, &len_field(1, &commands));
        assert!(body.len() <= MAX_CONSENSUS_GOSSIP_PAYLOAD_BYTES);

        let err = decode_gossip_payload(&framed(&body)).unwrap_err();
        assert!(err.to_string().contains("exceeds the decode budget"), "{err}");
    }
}
