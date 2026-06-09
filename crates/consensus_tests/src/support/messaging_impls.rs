//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus::{
    messages::HotstuffMessage,
    traits::{InboundMessaging, InboundMessagingError, OutboundMessaging, OutboundMessagingError},
};
use tokio::sync::mpsc;

use super::epoch_manager::TestEpochManager;
use crate::support::TestAddress;

#[derive(Debug, Clone)]
pub struct TestOutboundMessaging {
    epoch_manager: TestEpochManager,
    tx_leader: mpsc::Sender<(TestAddress, HotstuffMessage)>,
    tx_broadcast: mpsc::Sender<(Vec<TestAddress>, HotstuffMessage)>,
    loopback_sender: mpsc::Sender<HotstuffMessage>,
}

impl TestOutboundMessaging {
    pub fn create(
        epoch_manager: TestEpochManager,
        tx_leader: mpsc::Sender<(TestAddress, HotstuffMessage)>,
        tx_broadcast: mpsc::Sender<(Vec<TestAddress>, HotstuffMessage)>,
    ) -> (Self, mpsc::Receiver<HotstuffMessage>) {
        let (loopback_sender, loopback_receiver) = mpsc::channel(100);
        (
            Self {
                epoch_manager,
                tx_leader,
                tx_broadcast,
                loopback_sender,
            },
            loopback_receiver,
        )
    }
}

impl OutboundMessaging for TestOutboundMessaging {
    type Addr = TestAddress;

    async fn send_self<T: Into<HotstuffMessage> + Send>(&mut self, message: T) -> Result<(), OutboundMessagingError> {
        self.loopback_sender
            .send(message.into())
            .await
            .map_err(|_| OutboundMessagingError::FailedToEnqueueMessage {
                reason: "loopback channel closed".to_string(),
            })
    }

    async fn send<T: Into<HotstuffMessage> + Send>(
        &mut self,
        to: Self::Addr,
        message: T,
    ) -> Result<(), OutboundMessagingError> {
        self.tx_leader
            .send((to, message.into()))
            .await
            .map_err(|_| OutboundMessagingError::FailedToEnqueueMessage {
                reason: "leader channel closed".to_string(),
            })
    }

    async fn multicast<T, I>(&mut self, addresses: I, message: T) -> Result<(), OutboundMessagingError>
    where
        I: IntoIterator<Item = Self::Addr> + Send,
        T: Into<HotstuffMessage> + Send,
    {
        let peers = addresses.into_iter().collect();

        self.tx_broadcast.send((peers, message.into())).await.map_err(|_| {
            OutboundMessagingError::FailedToEnqueueMessage {
                reason: "broadcast channel closed".to_string(),
            }
        })
    }

    async fn broadcast<T>(&mut self, message: T) -> Result<(), OutboundMessagingError>
    where T: Into<HotstuffMessage> + Send {
        // A broadcast is gossiped on a single network-wide topic, so it reaches every validator. Messages relevant
        // only to a subset of shard groups carry that audience in their payload and are filtered by the receiver.
        let peers = self
            .epoch_manager
            .all_validators()
            .await
            .into_iter()
            .map(|(vn, _)| vn.address);
        self.multicast(peers, message).await
    }
}

pub struct TestInboundMessaging {
    local_address: TestAddress,
    receiver: mpsc::Receiver<(TestAddress, HotstuffMessage)>,
    loopback_receiver: mpsc::Receiver<HotstuffMessage>,
}

impl TestInboundMessaging {
    pub fn new(
        local_address: TestAddress,
        receiver: mpsc::Receiver<(TestAddress, HotstuffMessage)>,
        loopback_receiver: mpsc::Receiver<HotstuffMessage>,
    ) -> Self {
        Self {
            local_address,
            receiver,
            loopback_receiver,
        }
    }
}

impl InboundMessaging for TestInboundMessaging {
    type Addr = TestAddress;

    async fn next_message(&mut self) -> Option<Result<(Self::Addr, HotstuffMessage), InboundMessagingError>> {
        tokio::select! {
            msg = self.receiver.recv() => msg.map(Ok),
            msg = self.loopback_receiver.recv() => msg.map(|msg| Ok((self.local_address.clone(), msg))),
        }
    }
}
