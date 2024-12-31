//   Copyright 2024. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use log::*;
use tari_consensus::messages::HotstuffMessage;
use tari_dan_common_types::ShardGroup;
use tari_dan_p2p::{proto, TariMessagingSpec};
use tari_networking::{NetworkingHandle, NetworkingService};
use tari_swarm::messaging::{
    prost::{Message, ProstCodec},
    Codec,
};

use super::ConsensusGossipError;
use crate::p2p::services::consensus_gossip::service::shard_group_to_topic;

const LOG_TARGET: &str = "tari::validator_node::consensus_gossip";

#[derive(Debug, Clone)]
pub struct ConsensusGossipHandle {
    networking: NetworkingHandle<TariMessagingSpec>,
    codec: ProstCodec<proto::consensus::HotStuffMessage>,
}

impl ConsensusGossipHandle {
    pub(super) fn new(networking: NetworkingHandle<TariMessagingSpec>) -> Self {
        Self {
            networking,
            codec: ProstCodec::default(),
        }
    }

    pub async fn publish(
        &mut self,
        shard_group: ShardGroup,
        message: HotstuffMessage,
    ) -> Result<(), ConsensusGossipError> {
        let topic = shard_group_to_topic(shard_group);

        let message = proto::consensus::HotStuffMessage::from(&message);
        let encoded_len = message.encoded_len();
        let mut buf = Vec::with_capacity(encoded_len);

        debug!(
            target: LOG_TARGET,
            "GOSSIP PUBLISH: topic: {topic} Message size: {encoded_len} bytes",
        );
        self.codec
            .encode_to(&mut buf, message)
            .await
            .map_err(|e| ConsensusGossipError::InvalidMessage(e.into()))?;

        self.networking.publish_gossip(topic, buf).await?;

        Ok(())
    }
}
