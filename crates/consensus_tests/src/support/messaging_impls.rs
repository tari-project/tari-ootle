//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, sync::Arc};

use tari_consensus::{
    messages::HotstuffMessage,
    traits::{InboundMessaging, InboundMessagingError, OutboundMessaging, OutboundMessagingError},
};
use tokio::sync::mpsc;

use super::epoch_manager::TestEpochManager;
use crate::support::{TestAddress, TestStore};

/// Runs on the sending task with the message in hand and nothing sent yet, so a test can assert on
/// what the sender had already done by the time it sent — durable state in particular — without
/// racing the receiving side.
pub type SendObserver = Arc<dyn Fn(&HotstuffMessage) + Send + Sync>;

/// A [`SendObserver`] shared by every validator in a test, handed the sender's address and its own
/// state store. The builder binds the two per validator.
pub type NetworkSendObserver = Arc<dyn Fn(&TestAddress, &TestStore, &HotstuffMessage) + Send + Sync>;

#[derive(Clone)]
pub struct TestOutboundMessaging {
    epoch_manager: TestEpochManager,
    tx_leader: mpsc::Sender<(TestAddress, HotstuffMessage)>,
    tx_broadcast: mpsc::Sender<(Vec<TestAddress>, HotstuffMessage)>,
    loopback_sender: mpsc::Sender<HotstuffMessage>,
    observer: Option<SendObserver>,
}

impl fmt::Debug for TestOutboundMessaging {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestOutboundMessaging")
            .field("epoch_manager", &self.epoch_manager)
            .field("tx_leader", &self.tx_leader)
            .field("tx_broadcast", &self.tx_broadcast)
            .field("loopback_sender", &self.loopback_sender)
            .field("has_observer", &self.observer.is_some())
            .finish()
    }
}

impl TestOutboundMessaging {
    pub fn create(
        epoch_manager: TestEpochManager,
        tx_leader: mpsc::Sender<(TestAddress, HotstuffMessage)>,
        tx_broadcast: mpsc::Sender<(Vec<TestAddress>, HotstuffMessage)>,
        observer: Option<SendObserver>,
    ) -> (Self, mpsc::Receiver<HotstuffMessage>) {
        let (loopback_sender, loopback_receiver) = mpsc::channel(100);
        (
            Self {
                epoch_manager,
                tx_leader,
                tx_broadcast,
                loopback_sender,
                observer,
            },
            loopback_receiver,
        )
    }

    fn observe(&self, message: &HotstuffMessage) {
        if let Some(observer) = self.observer.as_ref() {
            observer(message);
        }
    }
}

impl OutboundMessaging for TestOutboundMessaging {
    type Addr = TestAddress;

    async fn send_self<T: Into<HotstuffMessage> + Send>(&mut self, message: T) -> Result<(), OutboundMessagingError> {
        let message = message.into();
        self.observe(&message);
        self.loopback_sender
            .send(message)
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
        let message = message.into();
        self.observe(&message);
        self.tx_leader
            .send((to, message))
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
        let message = message.into();
        self.observe(&message);

        self.tx_broadcast
            .send((peers, message))
            .await
            .map_err(|_| OutboundMessagingError::FailedToEnqueueMessage {
                reason: "broadcast channel closed".to_string(),
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
