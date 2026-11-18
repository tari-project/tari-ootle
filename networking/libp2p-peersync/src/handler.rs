//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::VecDeque,
    convert::Infallible,
    future::{Ready, ready},
    sync::Arc,
    task::{Context, Poll},
};

use async_semaphore::{Semaphore, SemaphoreGuardArc};
use libp2p::{
    InboundUpgrade,
    OutboundUpgrade,
    PeerId,
    Stream,
    StreamProtocol,
    core::UpgradeInfo,
    swarm::{
        ConnectionHandler,
        ConnectionHandlerEvent,
        StreamUpgradeError,
        SubstreamProtocol,
        handler::{
            ConnectionEvent,
            DialUpgradeError,
            FullyNegotiatedInbound,
            FullyNegotiatedOutbound,
            ListenUpgradeError,
        },
    },
};

use crate::{
    Config,
    EMPTY_QUEUE_SHRINK_THRESHOLD,
    MAX_MESSAGE_SIZE,
    SignedPeerRecord,
    behaviour::WantList,
    error::Error,
    event::Event,
    inbound_task::inbound_sync_task,
    outbound_task::outbound_request_want_list_task,
    proto,
    store::PeerStore,
};

pub(crate) type Framed<In, Out = In, S = Stream> = asynchronous_codec::Framed<S, quick_protobuf_codec::Codec<In, Out>>;
pub(crate) type FramedOutbound<S = Stream> = Framed<proto::Message, proto::WantPeerResponse, S>;
pub(crate) type FramedInbound<S = Stream> = Framed<proto::WantPeerResponse, proto::Message, S>;

#[derive(Debug)]
pub enum HandlerAction {
    PushLocalRecord(Arc<SignedPeerRecord>),
    WantPeers(Arc<WantList>),
}

pub struct Handler<TStore> {
    peer_id: PeerId,
    protocol: StreamProtocol,
    must_request_substream: bool,
    is_complete: bool,
    pending_local_peer_record: Option<Arc<SignedPeerRecord>>,
    failed_attempts: usize,
    current_want_list: Arc<WantList>,
    /// The want list the in-flight outbound sync sent. When that sync succeeds and `current_want_list` is still this
    /// list, `current_want_list` is cleared, so a follow-up sync carries only work that arrived after it started.
    in_flight_want_list: Option<Arc<WantList>>,
    pending_events: VecDeque<Event>,
    outbound_tasks: futures_bounded::FuturesSet<Event>,
    inbound_tasks: futures_bounded::FuturesSet<Event>,
    config: Config,
    store: TStore,
    semaphore: Arc<Semaphore>,
    /// Held from the outbound substream request until the outbound sync ends, so `Some` means an outbound sync is
    /// in flight on this connection.
    aquired: Option<SemaphoreGuardArc>,
}

impl<TStore: PeerStore> Handler<TStore> {
    pub fn new(
        peer_id: PeerId,
        store: TStore,
        protocol: StreamProtocol,
        config: &Config,
        want_list: WantList,
        semaphore: Arc<Semaphore>,
        pending_local_peer_record: Option<Arc<SignedPeerRecord>>,
    ) -> Self {
        Self {
            store,
            peer_id,
            protocol,
            is_complete: false,
            pending_local_peer_record,
            failed_attempts: 0,
            current_want_list: Arc::new(want_list),
            in_flight_want_list: None,
            pending_events: VecDeque::new(),
            must_request_substream: true,
            outbound_tasks: futures_bounded::FuturesSet::new(config.sync_timeout, 1),
            inbound_tasks: futures_bounded::FuturesSet::new(config.sync_timeout, config.max_concurrent_streams),
            config: config.clone(),
            semaphore,
            aquired: None,
        }
    }
}

impl<TStore> Handler<TStore>
where TStore: PeerStore
{
    fn on_listen_upgrade_error(&self, error: ListenUpgradeError<(), Protocol<StreamProtocol>>) {
        tracing::warn!("unexpected listen upgrade error: {:?}", error.error);
    }

    /// Schedules an outbound sync for new work, with a fresh retry budget. While a sync is in flight, the request
    /// waits until that sync ends.
    fn schedule_sync(&mut self) {
        self.is_complete = false;
        self.must_request_substream = true;
        self.failed_attempts = 0;
    }

    /// Runs `fut` as this connection's outbound sync of `current_want_list`.
    fn start_outbound_sync<F>(&mut self, fut: F)
    where F: Future<Output = Event> + Send + 'static {
        if self.outbound_tasks.try_push(fut).is_err() {
            self.aquired = None;
            tracing::warn!("Dropping outbound peer sync because we are at capacity");
            return;
        }
        self.in_flight_want_list = Some(self.current_want_list.clone());
    }

    /// Releases the sync permit and clears the synced want list after a successful outbound sync, and resets the
    /// retry budget.
    fn on_outbound_sync_succeeded(&mut self) {
        self.aquired = None;
        self.failed_attempts = 0;
        if let Some(synced) = self.in_flight_want_list.take() &&
            Arc::ptr_eq(&synced, &self.current_want_list)
        {
            self.current_want_list = Arc::default();
        }
        // Work scheduled while the sync was in flight still needs its own sync
        if !self.must_request_substream {
            self.is_complete = true;
        }
    }

    /// Releases the sync permit after a failed outbound sync and schedules a retry. Returns
    /// `Error::MaxFailedAttemptsReached` once `max_failure_retries` consecutive attempts have failed, after which the
    /// handler stays idle until new work arrives.
    fn on_outbound_sync_failed(&mut self) -> Option<Error> {
        self.aquired = None;
        self.in_flight_want_list = None;
        self.failed_attempts += 1;
        if self.failed_attempts > self.config.max_failure_retries {
            self.is_complete = true;
            self.must_request_substream = false;
            Some(Error::MaxFailedAttemptsReached)
        } else {
            self.must_request_substream = true;
            None
        }
    }

    fn on_dial_upgrade_error(&mut self, error: DialUpgradeError<(), Protocol<StreamProtocol>>) {
        match error.error {
            StreamUpgradeError::Timeout => {
                self.pending_events.push_back(Event::OutboundFailure {
                    peer_id: self.peer_id,
                    error: Error::DialUpgradeError,
                });
                if let Some(err) = self.on_outbound_sync_failed() {
                    self.pending_events.push_back(Event::Error(err));
                }
            },
            StreamUpgradeError::NegotiationFailed => {
                // The remote merely doesn't support the protocol(s) we requested.
                // This is no reason to close the connection, which may
                // successfully communicate with other protocols already.
                // An event is reported to permit user code to react to the fact that
                // the remote peer does not support the requested protocol(s).
                self.aquired = None;
                self.is_complete = true;
                self.pending_events.push_back(Event::OutboundFailure {
                    peer_id: self.peer_id,
                    error: Error::ProtocolNotSupported,
                });
            },
            StreamUpgradeError::Apply(_) => {},
            StreamUpgradeError::Io(e) => {
                tracing::debug!("outbound stream for request failed: {e}, retrying",);
                self.aquired = None;
                self.must_request_substream = true;
            },
        }
    }

    fn on_fully_negotiated_outbound(&mut self, outbound: FullyNegotiatedOutbound<Protocol<StreamProtocol>, ()>) {
        if self.current_want_list.is_empty() && self.pending_local_peer_record.is_none() {
            tracing::trace!("No peers wanted or no local peer record update, ignoring outbound stream");
            self.aquired = None;
            return;
        }
        tracing::debug!(
            "peer-sync[{}]: Starting outbound protocol on outbound stream",
            self.peer_id
        );
        let (stream, _protocol) = outbound.protocol;
        let framed = new_framed_codec(stream, MAX_MESSAGE_SIZE);
        let store = self.store.clone();
        let config = self.config.clone();

        let pending_local_peer_record = self.pending_local_peer_record.take();
        let fut = outbound_request_want_list_task(
            self.peer_id,
            framed,
            store,
            self.current_want_list.clone(),
            pending_local_peer_record,
            config,
        );
        self.start_outbound_sync(fut);
    }

    fn on_fully_negotiated_inbound(&mut self, inbound: FullyNegotiatedInbound<Protocol<StreamProtocol>, ()>) {
        let (stream, _protocol) = inbound.protocol;
        let config = self.config.clone();
        let framed = new_framed_codec(stream, MAX_MESSAGE_SIZE);
        let store = self.store.clone();

        let fut = inbound_sync_task(self.peer_id, framed, store, config);

        if self.inbound_tasks.try_push(fut).is_err() {
            tracing::warn!("Dropping inbound peer sync because we are at capacity")
        }
    }
}

impl<TStore> ConnectionHandler for Handler<TStore>
where TStore: PeerStore
{
    type FromBehaviour = HandlerAction;
    type InboundOpenInfo = ();
    type InboundProtocol = Protocol<StreamProtocol>;
    type OutboundOpenInfo = ();
    type OutboundProtocol = Protocol<StreamProtocol>;
    type ToBehaviour = Event;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(
            Protocol {
                protocol: self.protocol.clone(),
            },
            (),
        )
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>> {
        match self.outbound_tasks.poll_unpin(cx) {
            Poll::Ready(Ok(event @ Event::PeerBatchReceived { .. })) => {
                self.on_outbound_sync_succeeded();
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
            },
            Poll::Ready(Ok(event)) => {
                if let Some(err) = self.on_outbound_sync_failed() {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::Error(err)));
                }
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
            },
            Poll::Ready(Err(timeout)) => {
                if let Some(err) = self.on_outbound_sync_failed() {
                    self.pending_events.push_back(Event::Error(err));
                }
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::Error(Error::Timeout(
                    timeout,
                ))));
            },
            Poll::Pending => {},
        }

        match self.inbound_tasks.poll_unpin(cx) {
            Poll::Ready(Ok(event)) => return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event)),
            Poll::Ready(Err(timeout)) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::Error(Error::Timeout(
                    timeout,
                ))));
            },
            Poll::Pending => {},
        }

        // Drain pending events that were produced by handler
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }
        if self.pending_events.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_events.shrink_to_fit();
        }

        // If we've synced from the peer already or if the sync failed, there's nothing further to do
        if self.is_complete {
            return Poll::Pending;
        }

        // If we do not want any peers, there's nothing further to do
        if self.current_want_list.is_empty() && self.pending_local_peer_record.is_none() {
            return Poll::Pending;
        }
        tracing::debug!(
            "peer-sync[{}]: Want {} peers, has pending local peer update = {}",
            self.peer_id,
            self.current_want_list.len(),
            self.pending_local_peer_record.is_some()
        );

        // Our turn, once this connection's own outbound sync has ended
        if self.must_request_substream && self.aquired.is_none() {
            // Wait until another sync is complete
            match self.semaphore.try_acquire_arc() {
                Some(guard) => {
                    self.aquired = Some(guard);
                },
                None => {
                    return Poll::Pending;
                },
            }

            tracing::debug!("peer-sync[{}]: Acquired semaphore", self.peer_id);

            let protocol = self.protocol.clone();
            self.must_request_substream = false;

            tracing::debug!("peer-sync[{}]: Requesting substream open", self.peer_id);
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(Protocol { protocol }, ()),
            });
        }

        Poll::Pending
    }

    fn on_behaviour_event(&mut self, action: Self::FromBehaviour) {
        match action {
            HandlerAction::PushLocalRecord(local_record) => {
                self.pending_local_peer_record = Some(local_record);
                self.schedule_sync();
            },
            HandlerAction::WantPeers(want_list) => {
                // Sync from existing connections if there are more want-peers
                let has_wanted_peers = !want_list.is_empty();
                self.current_want_list = want_list;
                if has_wanted_peers {
                    self.schedule_sync();
                }
            },
        }
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                self.on_fully_negotiated_inbound(fully_negotiated_inbound)
            },
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                self.on_fully_negotiated_outbound(fully_negotiated_outbound)
            },
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => self.on_dial_upgrade_error(dial_upgrade_error),
            ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                self.on_listen_upgrade_error(listen_upgrade_error)
            },
            _ => {},
        }
    }
}

pub struct Protocol<P> {
    pub(crate) protocol: P,
}

impl<P> UpgradeInfo for Protocol<P>
where P: AsRef<str> + Clone
{
    type Info = P;
    type InfoIter = std::option::IntoIter<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        Some(self.protocol.clone()).into_iter()
    }
}

impl<P> InboundUpgrade<Stream> for Protocol<P>
where P: AsRef<str> + Clone
{
    type Error = Infallible;
    type Future = Ready<Result<Self::Output, Self::Error>>;
    type Output = (Stream, P);

    fn upgrade_inbound(self, io: Stream, protocol: Self::Info) -> Self::Future {
        ready(Ok((io, protocol)))
    }
}

impl<P> OutboundUpgrade<Stream> for Protocol<P>
where P: AsRef<str> + Clone
{
    type Error = Infallible;
    type Future = Ready<Result<Self::Output, Self::Error>>;
    type Output = (Stream, P);

    fn upgrade_outbound(self, io: Stream, protocol: Self::Info) -> Self::Future {
        ready(Ok((io, protocol)))
    }
}

fn new_framed_codec<In: quick_protobuf::MessageWrite, Out: for<'a> quick_protobuf::MessageRead<'a>>(
    stream: Stream,
    max_message_length: usize,
) -> Framed<In, Out> {
    asynchronous_codec::Framed::new(stream, quick_protobuf_codec::Codec::<In, Out>::new(max_message_length))
}

#[cfg(test)]
mod tests {
    use std::{future, time::Duration};

    use libp2p::{
        futures::{executor::block_on, future::poll_fn, task::noop_waker_ref},
        identity,
    };

    use super::*;
    use crate::{LocalPeerRecord, store::MemoryPeerStore};

    type TestHandler = Handler<MemoryPeerStore>;
    type HandlerPoll = Poll<ConnectionHandlerEvent<Protocol<StreamProtocol>, (), Event>>;

    fn new_handler(config: &Config, want_list: WantList, semaphore: Arc<Semaphore>) -> TestHandler {
        Handler::new(
            PeerId::random(),
            MemoryPeerStore::new(),
            crate::DEFAULT_PROTOCOL_NAME,
            config,
            want_list,
            semaphore,
            None,
        )
    }

    fn local_record() -> Arc<SignedPeerRecord> {
        let mut rec = LocalPeerRecord::new(Arc::new(identity::Keypair::generate_ed25519()));
        rec.add_address("/ip4/127.0.0.1/tcp/9200".parse().unwrap());
        Arc::new(rec.into())
    }

    fn poll_once(handler: &mut TestHandler) -> HandlerPoll {
        handler.poll(&mut Context::from_waker(noop_waker_ref()))
    }

    fn is_substream_request(poll: &HandlerPoll) -> bool {
        matches!(
            poll,
            Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { .. })
        )
    }

    fn complete_outbound_sync(handler: &mut TestHandler, event: Event) -> HandlerPoll {
        handler.start_outbound_sync(future::ready(event));
        assert!(handler.in_flight_want_list.is_some());
        poll_once(handler)
    }

    fn peer_batch(new_peers: usize) -> Event {
        Event::PeerBatchReceived {
            from_peer: PeerId::random(),
            new_peers,
        }
    }

    #[test]
    fn want_peers_schedules_sync() {
        let semaphore = Arc::new(Semaphore::new(1));
        let mut handler = new_handler(&Config::default(), WantList::new(), semaphore);
        assert!(poll_once(&mut handler).is_pending());

        handler.on_behaviour_event(HandlerAction::WantPeers(Arc::new(
            [PeerId::random()].into_iter().collect(),
        )));

        assert!(!handler.is_complete);
        assert!(is_substream_request(&poll_once(&mut handler)));
    }

    #[test]
    fn work_during_sync_waits_for_it_then_resyncs() {
        let semaphore = Arc::new(Semaphore::new(1));
        let mut handler = new_handler(
            &Config::default(),
            [PeerId::random()].into_iter().collect(),
            semaphore.clone(),
        );
        assert!(is_substream_request(&poll_once(&mut handler)));

        handler.on_behaviour_event(HandlerAction::PushLocalRecord(local_record()));
        handler.on_behaviour_event(HandlerAction::WantPeers(Arc::new(
            [PeerId::random()].into_iter().collect(),
        )));
        assert!(poll_once(&mut handler).is_pending());
        assert!(semaphore.try_acquire().is_none());

        assert!(complete_outbound_sync(&mut handler, peer_batch(1)).is_ready());
        assert!(!handler.is_complete);
        assert!(is_substream_request(&poll_once(&mut handler)));
        assert!(semaphore.try_acquire().is_none());
    }

    #[test]
    fn empty_want_list_during_sync_keeps_permit() {
        let semaphore = Arc::new(Semaphore::new(1));
        let mut handler = new_handler(
            &Config::default(),
            [PeerId::random()].into_iter().collect(),
            semaphore.clone(),
        );
        assert!(is_substream_request(&poll_once(&mut handler)));

        handler.on_behaviour_event(HandlerAction::WantPeers(Arc::new(WantList::new())));
        assert!(poll_once(&mut handler).is_pending());
        assert!(semaphore.try_acquire().is_none());
    }

    #[test]
    fn peer_batch_completes_sync_and_releases_permit() {
        let semaphore = Arc::new(Semaphore::new(1));
        let mut handler = new_handler(
            &Config::default(),
            [PeerId::random()].into_iter().collect(),
            semaphore.clone(),
        );
        assert!(is_substream_request(&poll_once(&mut handler)));

        assert!(complete_outbound_sync(&mut handler, peer_batch(0)).is_ready());
        assert!(handler.is_complete);
        assert!(poll_once(&mut handler).is_pending());
        assert!(semaphore.try_acquire().is_some());
    }

    #[test]
    fn outbound_task_timeout_counts_failure_and_retries() {
        let config = Config {
            sync_timeout: Duration::from_millis(1),
            ..Config::default()
        };
        let semaphore = Arc::new(Semaphore::new(1));
        let mut handler = new_handler(&config, [PeerId::random()].into_iter().collect(), semaphore.clone());
        assert!(is_substream_request(&poll_once(&mut handler)));

        assert!(handler.outbound_tasks.try_push(future::pending()).is_ok());
        let event = block_on(poll_fn(|cx| handler.poll(cx)));
        assert!(matches!(
            event,
            ConnectionHandlerEvent::NotifyBehaviour(Event::Error(Error::Timeout(_)))
        ));
        assert_eq!(handler.failed_attempts, 1);
        assert!(handler.aquired.is_none());
        assert!(is_substream_request(&poll_once(&mut handler)));
    }

    #[test]
    fn dial_upgrade_timeout_counts_failure_and_gives_up_after_max_retries() {
        let config = Config {
            max_failure_retries: 1,
            ..Config::default()
        };
        let semaphore = Arc::new(Semaphore::new(1));
        let mut handler = new_handler(&config, [PeerId::random()].into_iter().collect(), semaphore.clone());

        for attempt in 1..=config.max_failure_retries + 1 {
            assert!(is_substream_request(&poll_once(&mut handler)));
            handler.on_dial_upgrade_error(DialUpgradeError {
                info: (),
                error: StreamUpgradeError::Timeout,
            });
            assert_eq!(handler.failed_attempts, attempt);
            assert!(semaphore.try_acquire().is_some());
            assert!(matches!(
                poll_once(&mut handler),
                Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::OutboundFailure { .. }))
            ));
        }

        assert!(handler.is_complete);
        assert!(matches!(
            poll_once(&mut handler),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::Error(
                Error::MaxFailedAttemptsReached
            )))
        ));
        assert!(poll_once(&mut handler).is_pending());
    }

    #[test]
    fn inbound_task_timeout_keeps_outbound_permit() {
        let config = Config {
            sync_timeout: Duration::from_millis(1),
            ..Config::default()
        };
        let semaphore = Arc::new(Semaphore::new(1));
        let mut handler = new_handler(&config, [PeerId::random()].into_iter().collect(), semaphore.clone());
        assert!(is_substream_request(&poll_once(&mut handler)));

        assert!(handler.inbound_tasks.try_push(future::pending()).is_ok());
        let event = block_on(poll_fn(|cx| handler.poll(cx)));
        assert!(matches!(
            event,
            ConnectionHandlerEvent::NotifyBehaviour(Event::Error(Error::Timeout(_)))
        ));
        assert_eq!(handler.failed_attempts, 0);
        assert!(handler.aquired.is_some());
        assert!(semaphore.try_acquire().is_none());
    }

    #[test]
    fn local_record_pushed_during_sync_resyncs_without_synced_want_list() {
        let semaphore = Arc::new(Semaphore::new(1));
        let mut handler = new_handler(&Config::default(), [PeerId::random()].into_iter().collect(), semaphore);
        assert!(is_substream_request(&poll_once(&mut handler)));

        handler.start_outbound_sync(future::ready(peer_batch(0)));
        handler.on_behaviour_event(HandlerAction::PushLocalRecord(local_record()));
        assert!(poll_once(&mut handler).is_ready());

        assert!(handler.current_want_list.is_empty());
        assert!(handler.pending_local_peer_record.is_some());
        assert!(is_substream_request(&poll_once(&mut handler)));
    }

    #[test]
    fn want_list_replaced_during_sync_survives_sync() {
        let semaphore = Arc::new(Semaphore::new(1));
        let mut handler = new_handler(&Config::default(), [PeerId::random()].into_iter().collect(), semaphore);
        assert!(is_substream_request(&poll_once(&mut handler)));

        handler.start_outbound_sync(future::ready(peer_batch(0)));
        let newer = Arc::new([PeerId::random()].into_iter().collect::<WantList>());
        handler.on_behaviour_event(HandlerAction::WantPeers(newer.clone()));
        assert!(poll_once(&mut handler).is_ready());

        assert!(Arc::ptr_eq(&handler.current_want_list, &newer));
        assert!(is_substream_request(&poll_once(&mut handler)));
    }

    #[test]
    fn success_and_new_work_reset_failed_attempts() {
        let config = Config {
            max_failure_retries: 0,
            ..Config::default()
        };
        let semaphore = Arc::new(Semaphore::new(1));
        let mut handler = new_handler(&config, [PeerId::random()].into_iter().collect(), semaphore);

        assert!(is_substream_request(&poll_once(&mut handler)));
        handler.on_dial_upgrade_error(DialUpgradeError {
            info: (),
            error: StreamUpgradeError::Timeout,
        });
        assert!(handler.is_complete);

        handler.on_behaviour_event(HandlerAction::WantPeers(Arc::new(
            [PeerId::random()].into_iter().collect(),
        )));
        assert_eq!(handler.failed_attempts, 0);
        assert!(!handler.is_complete);

        handler.failed_attempts = 1;
        assert!(complete_outbound_sync(&mut handler, peer_batch(0)).is_ready());
        assert_eq!(handler.failed_attempts, 0);
    }
}
