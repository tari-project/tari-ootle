//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

use libp2p::PeerId;
use tari_consensus::{messages::HotstuffMessage, traits::InboundMessagingError};
use tari_epoch_manager::{EpochManagerReader, service::EpochManagerHandle};
use tari_networking::InboundMessage;
use tari_ootle_common_types::Epoch;
use tari_ootle_p2p::{PeerAddress, proto};
use tokio::sync::mpsc;

use crate::p2p::logging::MessageLogger;

/// Answers whether a peer is a validator registered for an epoch.
pub trait ValidatorRegistry {
    fn is_registered(&self, epoch: Epoch, address: &PeerAddress) -> impl Future<Output = bool> + Send;
}

impl ValidatorRegistry for EpochManagerHandle<PeerAddress> {
    async fn is_registered(&self, epoch: Epoch, address: &PeerAddress) -> bool {
        self.get_committee_info_by_validator_address(epoch, address)
            .await
            .is_ok()
    }
}

pub struct ConsensusInboundMessaging<TMsgLogger, TRegistry = EpochManagerHandle<PeerAddress>> {
    local_address: PeerAddress,
    rx_inbound_msg: mpsc::Receiver<InboundMessage<proto::consensus::HotStuffMessage>>,
    rx_gossip: mpsc::Receiver<(PeerId, HotstuffMessage)>,
    rx_loopback: mpsc::UnboundedReceiver<HotstuffMessage>,
    msg_logger: TMsgLogger,
    validators: TRegistry,
    /// A direct message taken off the queue whose sender is still being checked. It is held here rather than
    /// in the `next_message` future, so a caller that drops that future part way does not lose it.
    awaiting_sender_check: Option<(PeerId, proto::consensus::HotStuffMessage)>,
}

impl<TMsgLogger: MessageLogger, TRegistry: ValidatorRegistry> ConsensusInboundMessaging<TMsgLogger, TRegistry> {
    pub fn new(
        local_address: PeerAddress,
        rx_inbound_msg: mpsc::Receiver<InboundMessage<proto::consensus::HotStuffMessage>>,
        rx_gossip: mpsc::Receiver<(PeerId, HotstuffMessage)>,
        rx_loopback: mpsc::UnboundedReceiver<HotstuffMessage>,
        msg_logger: TMsgLogger,
        validators: TRegistry,
    ) -> Self {
        Self {
            local_address,
            rx_inbound_msg,
            rx_gossip,
            rx_loopback,
            msg_logger,
            validators,
            awaiting_sender_check: None,
        }
    }

    fn handle_message(
        &self,
        from: PeerId,
        msg: proto::consensus::HotStuffMessage,
    ) -> Option<Result<(PeerAddress, HotstuffMessage), InboundMessagingError>> {
        match HotstuffMessage::try_from(msg) {
            Ok(msg) => {
                self.msg_logger
                    .log_inbound_message(&from.to_string(), msg.as_type_str(), "", &msg);
                Some(Ok((from.into(), msg)))
            },
            Err(err) => Some(Err(InboundMessagingError::InvalidMessage {
                reason: format!("from peer {from}: {err}"),
            })),
        }
    }
}

impl<TMsgLogger, TRegistry> tari_consensus::traits::InboundMessaging
    for ConsensusInboundMessaging<TMsgLogger, TRegistry>
where
    TMsgLogger: MessageLogger + Send,
    TRegistry: ValidatorRegistry + Send + Sync,
{
    type Addr = PeerAddress;

    async fn next_message(&mut self) -> Option<Result<(Self::Addr, HotstuffMessage), InboundMessagingError>> {
        if self.awaiting_sender_check.is_none() {
            tokio::select! {
                // BIASED: messaging priority is loopback, then other
                biased;
                maybe_msg = self.rx_loopback.recv() => return maybe_msg.map(|msg| {
                    self.msg_logger.log_inbound_message(
                       &self.local_address.to_string(),
                       msg.as_type_str(),
                       "",
                       &msg,
                    );
                    Ok((self.local_address, msg))
                }),
                maybe_msg = self.rx_inbound_msg.recv() => {
                    let inbound = maybe_msg?;
                    if validator_only_epoch(&inbound.message).is_none() {
                        return self.handle_message(inbound.peer_id, inbound.message);
                    }
                    self.awaiting_sender_check = Some((inbound.peer_id, inbound.message));
                },
                maybe_msg = self.rx_gossip.recv() => {
                    let (from, msg) = maybe_msg?;
                    self.msg_logger
                        .log_inbound_message(&from.to_string(), msg.as_type_str(), "", &msg);
                    return Some(Ok((from.into(), msg)));
                },
            }
        }

        match resolve_sender_check(&mut self.awaiting_sender_check, &self.validators).await? {
            Ok((from, msg)) => self.handle_message(from, msg),
            Err(err) => Some(Err(err)),
        }
    }
}

/// The epoch whose registered validators alone may send `msg`, for a kind only a validator sends. A
/// transactions response is only ever requested from a validator, and its transactions are costly to decode, so
/// its sender is checked before they are.
fn validator_only_epoch(msg: &proto::consensus::HotStuffMessage) -> Option<Epoch> {
    match &msg.message {
        Some(proto::consensus::hot_stuff_message::Message::RequestedTransaction(response)) => {
            Some(Epoch(response.epoch))
        },
        _ => None,
    }
}

/// Checks the sender of the message held in `pending`, releasing it only once the check completes. A caller that
/// drops this future part way leaves the message in `pending` for the next call to check again.
async fn resolve_sender_check<TRegistry: ValidatorRegistry>(
    pending: &mut Option<(PeerId, proto::consensus::HotStuffMessage)>,
    validators: &TRegistry,
) -> Option<Result<(PeerId, proto::consensus::HotStuffMessage), InboundMessagingError>> {
    let (from, msg) = pending.as_ref()?;
    let rejection = match validator_only_epoch(msg) {
        Some(epoch) if !validators.is_registered(epoch, &(*from).into()).await => Some(epoch),
        _ => None,
    };
    let (from, msg) = pending.take()?;
    match rejection {
        Some(epoch) => Some(Err(InboundMessagingError::InvalidMessage {
            reason: format!("peer {from} sent transactions for {epoch} but is not a validator registered for it"),
        })),
        None => Some(Ok((from, msg))),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };

    use tari_ootle_p2p::proto::{
        consensus::{HotStuffMessage, MissingTransactionsResponse, hot_stuff_message::Message},
        transaction::Transaction,
    };

    use super::*;

    struct Registered(HashSet<PeerAddress>);

    impl ValidatorRegistry for Registered {
        async fn is_registered(&self, _epoch: Epoch, address: &PeerAddress) -> bool {
            self.0.contains(address)
        }
    }

    /// Never answers its first lookup, then answers every later one as registered.
    struct StallsOnce(AtomicBool);

    impl ValidatorRegistry for StallsOnce {
        async fn is_registered(&self, _epoch: Epoch, _address: &PeerAddress) -> bool {
            if !self.0.swap(true, Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            true
        }
    }

    async fn check_sender<TRegistry: ValidatorRegistry>(
        validators: &TRegistry,
        from: PeerId,
        msg: HotStuffMessage,
    ) -> Result<(PeerId, HotStuffMessage), InboundMessagingError> {
        resolve_sender_check(&mut Some((from, msg)), validators)
            .await
            .expect("a message is pending")
    }

    fn transactions_response() -> HotStuffMessage {
        HotStuffMessage {
            message: Some(Message::RequestedTransaction(MissingTransactionsResponse {
                request_id: 1,
                epoch: 1,
                block_id: vec![1; 32],
                transactions: vec![Transaction {
                    bor_encoded: vec![0xff; 16],
                }],
            })),
        }
    }

    #[tokio::test]
    async fn a_transactions_response_from_an_unregistered_peer_is_refused() {
        let validator = PeerId::random();
        let registry = Registered(HashSet::from([validator.into()]));

        let err = check_sender(&registry, PeerId::random(), transactions_response())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not a validator"), "{err}");
    }

    #[tokio::test]
    async fn a_transactions_response_from_a_registered_validator_is_admitted() {
        let validator = PeerId::random();
        let registry = Registered(HashSet::from([validator.into()]));

        check_sender(&registry, validator, transactions_response())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn other_messages_are_admitted_from_any_peer() {
        let registry = Registered(HashSet::new());
        let request = HotStuffMessage {
            message: Some(Message::RequestMissingTransactions(Default::default())),
        };

        check_sender(&registry, PeerId::random(), request).await.unwrap();
    }

    #[tokio::test]
    async fn a_check_dropped_part_way_keeps_the_message_for_the_next_call() {
        let registry = StallsOnce(AtomicBool::new(false));
        let from = PeerId::random();
        let mut pending = Some((from, transactions_response()));

        let dropped =
            tokio::time::timeout(Duration::from_millis(10), resolve_sender_check(&mut pending, &registry)).await;
        assert!(dropped.is_err(), "the first lookup never answers");
        assert!(pending.is_some(), "the message survives the dropped check");

        let (resolved_from, _) = resolve_sender_check(&mut pending, &registry)
            .await
            .expect("the message is still pending")
            .unwrap();
        assert_eq!(resolved_from, from);
        assert!(pending.is_none());
    }
}
