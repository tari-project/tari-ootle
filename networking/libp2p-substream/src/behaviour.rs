//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, VecDeque},
    task::{Context, Poll},
};

use libp2p::{
    Multiaddr,
    PeerId,
    StreamProtocol,
    core::{Endpoint, transport::PortUse},
    swarm::{
        AddressChange,
        ConnectionClosed,
        ConnectionDenied,
        ConnectionHandler,
        ConnectionId,
        DialError,
        DialFailure,
        FromSwarm,
        NetworkBehaviour,
        NotifyHandler,
        THandler,
        THandlerInEvent,
        THandlerOutEvent,
        ToSwarm,
        dial_opts::DialOpts,
    },
};
use smallvec::SmallVec;

use crate::{
    Config,
    FromBehaviourEvent,
    OpenStreamRequest,
    error::Error,
    event::Event,
    handler::Handler,
    stream::StreamId,
};

/// Internal threshold for when to shrink the capacity
/// of empty queues. If the capacity of an empty queue
/// exceeds this threshold, the associated memory is
/// released.
pub const EMPTY_QUEUE_SHRINK_THRESHOLD: usize = 100;

#[derive(Debug)]
pub struct Behaviour {
    protocols: SmallVec<[StreamProtocol; 32]>,
    pending_events: VecDeque<ToSwarm<Event, THandlerInEvent<Self>>>,
    /// The currently connected peers, their pending outbound and inbound responses and their known,
    /// reachable addresses, if any.
    connected: HashMap<PeerId, SmallVec<[Connection; 2]>>,
    pending_outbound_streams: HashMap<PeerId, SmallVec<[OpenStreamRequest; 10]>>,
    next_stream_id: StreamId,
}

impl Behaviour {
    pub fn new<I: IntoIterator<Item = StreamProtocol>>(protocols: I, _config: Config) -> Self {
        let mut unique_protocols = SmallVec::new();
        for protocol in protocols {
            if !unique_protocols.contains(&protocol) {
                unique_protocols.push(protocol);
            }
        }
        Self {
            protocols: unique_protocols,
            pending_events: VecDeque::new(),
            pending_outbound_streams: HashMap::new(),
            connected: HashMap::new(),
            next_stream_id: StreamId::default(),
        }
    }

    pub fn add_protocol(&mut self, protocol: StreamProtocol) {
        if self.protocols.contains(&protocol) {
            return;
        }
        self.protocols.push(protocol.clone());
        // Notify all active connections
        for (peer_id, connections) in &self.connected {
            for conn in connections {
                self.pending_events.push_back(ToSwarm::NotifyHandler {
                    peer_id: *peer_id,
                    handler: NotifyHandler::One(conn.id),
                    event: FromBehaviourEvent::AddSupportedProtocol(protocol.clone()),
                });
            }
        }
    }

    pub fn supported_protocols(&self) -> &[StreamProtocol] {
        self.protocols.as_ref()
    }

    pub fn open_substream(&mut self, peer_id: PeerId, protocol: StreamProtocol) -> StreamId {
        let stream_id = self.next_stream_id();
        let request = OpenStreamRequest::new(stream_id, peer_id, protocol.clone());

        match self.get_connections(&peer_id) {
            Some(connections) => {
                let ix = (stream_id as usize) % connections.len();
                let conn = connections.get_mut(ix).expect("mod operator ensures index is in range");
                conn.pending_streams.insert(stream_id, protocol);
                let conn_id = conn.id;
                self.pending_events.push_back(ToSwarm::NotifyHandler {
                    peer_id,
                    handler: NotifyHandler::One(conn_id),
                    event: request.into(),
                });
            },
            None => {
                if !self.pending_outbound_streams.contains_key(&peer_id) {
                    self.pending_events.push_back(ToSwarm::Dial {
                        opts: DialOpts::peer_id(peer_id).build(),
                    });
                }
                self.pending_outbound_streams.entry(peer_id).or_default().push(request);
            },
        }
        stream_id
    }

    fn next_stream_id(&mut self) -> StreamId {
        let request_id = self.next_stream_id;
        self.next_stream_id = self.next_stream_id.wrapping_add(1);
        request_id
    }

    fn on_connection_closed(
        &mut self,
        ConnectionClosed {
            peer_id,
            connection_id,
            remaining_established,
            ..
        }: ConnectionClosed,
    ) {
        let Some(connections) = self.connected.get_mut(&peer_id) else {
            tracing::warn!(
                "Expected some established connection to peer before closing (peer_id={peer_id}, \
                 connection_id={connection_id})"
            );
            return;
        };

        let Some(connection) = connections
            .iter()
            .position(|c| c.id == connection_id)
            .map(|p: usize| connections.remove(p))
        else {
            tracing::warn!(
                "Expected connection to be established before closing (peer_id={peer_id}, \
                 connection_id={connection_id})"
            );
            return;
        };

        debug_assert_eq!(connections.is_empty(), remaining_established == 0);
        if connections.is_empty() {
            self.connected.remove(&peer_id);
        }

        for (stream_id, protocol) in connection.pending_streams {
            self.pending_events
                .push_back(ToSwarm::GenerateEvent(Event::OutboundFailure {
                    peer_id,
                    protocol,
                    stream_id,
                    error: Error::ConnectionClosed,
                }));
        }
    }

    fn on_address_change(&mut self, address_change: AddressChange) {
        let AddressChange {
            peer_id,
            connection_id,
            new,
            ..
        } = address_change;
        if let Some(connections) = self.connected.get_mut(&peer_id) {
            for connection in connections {
                if connection.id == connection_id {
                    connection.remote_address = Some(new.get_remote_address().clone());
                    return;
                }
            }
        }
    }

    fn on_dial_failure(&mut self, DialFailure { peer_id, error, .. }: DialFailure) {
        if matches!(error, DialError::DialPeerConditionFalse(_)) {
            return;
        }

        if let Some(peer) = peer_id {
            // If there are pending outgoing stream requests when a dial failure occurs,
            // it is implied that we are not connected to the peer, since pending
            // outgoing stream requests are drained when a connection is established and
            // only created when a peer is not connected when a request is made.
            // Therefore these requests must be considered failed, even if there is
            // another, concurrent dialing attempt ongoing.
            if let Some(pending) = self.pending_outbound_streams.remove(&peer) {
                let no_addresses = matches!(&error, DialError::NoAddresses);
                for request in pending {
                    self.pending_events
                        .push_back(ToSwarm::GenerateEvent(Event::OutboundFailure {
                            peer_id: peer,
                            protocol: request.protocol().clone(),
                            stream_id: request.stream_id(),
                            error: if no_addresses {
                                Error::NoAddressesForPeer
                            } else {
                                Error::DialFailure {
                                    details: error.to_string(),
                                }
                            },
                        }));
                }
            }
        }
    }

    fn on_connection_established(
        &mut self,
        handler: &mut Handler,
        peer_id: PeerId,
        connection_id: ConnectionId,
        remote_address: Option<Multiaddr>,
    ) {
        let mut connection = Connection::new(connection_id, remote_address);

        if let Some(pending_streams) = self.pending_outbound_streams.remove(&peer_id) {
            for stream in pending_streams {
                connection
                    .pending_streams
                    .insert(stream.stream_id(), stream.protocol().clone());
                handler.on_behaviour_event(stream.into());
            }
        }

        self.connected.entry(peer_id).or_default().push(connection);
    }

    fn get_connections(&mut self, peer_id: &PeerId) -> Option<&mut SmallVec<[Connection; 2]>> {
        self.connected.get_mut(peer_id).filter(|c| !c.is_empty())
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        let mut handler = Handler::new(peer, self.protocols.clone());
        self.on_connection_established(&mut handler, peer, connection_id, Some(remote_addr.clone()));

        Ok(handler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        remote_addr: &Multiaddr,
        _role_override: Endpoint,
        _port_use: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        let mut handler = Handler::new(peer, self.protocols.clone());
        self.on_connection_established(&mut handler, peer, connection_id, Some(remote_addr.clone()));
        Ok(handler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionEstablished(_) => {},
            FromSwarm::ConnectionClosed(connection_closed) => self.on_connection_closed(connection_closed),
            FromSwarm::AddressChange(address_change) => self.on_address_change(address_change),
            FromSwarm::DialFailure(dial_failure) => self.on_dial_failure(dial_failure),
            _ => {},
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match &event {
            Event::SubstreamOpen { peer_id, stream_id, .. } | Event::OutboundFailure { peer_id, stream_id, .. } => {
                if let Some(connections) = self.connected.get_mut(peer_id) {
                    for connection in connections {
                        connection.pending_streams.remove(stream_id);
                    }
                }
            },
            Event::InboundSubstreamOpen { .. } => {},
        }

        self.pending_events.push_back(ToSwarm::GenerateEvent(event));
    }

    fn poll(&mut self, _cx: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }
        if self.pending_events.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_events.shrink_to_fit();
        }

        Poll::Pending
    }
}

/// Internal information tracked for an established connection.
#[derive(Debug)]
struct Connection {
    id: ConnectionId,
    remote_address: Option<Multiaddr>,
    pending_streams: HashMap<StreamId, StreamProtocol>,
}

impl Connection {
    fn new(id: ConnectionId, remote_address: Option<Multiaddr>) -> Self {
        Self {
            id,
            remote_address,
            pending_streams: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{slice, task::Waker};

    use libp2p::{core::ConnectedPoint, swarm::ConnectionHandlerEvent};

    use super::*;

    fn drain(behaviour: &mut Behaviour) -> Vec<ToSwarm<Event, FromBehaviourEvent>> {
        let mut cx = Context::from_waker(Waker::noop());
        let mut events = Vec::new();
        while let Poll::Ready(event) = behaviour.poll(&mut cx) {
            events.push(event);
        }
        events
    }

    fn address() -> Multiaddr {
        "/ip4/127.0.0.1/tcp/18000".parse().unwrap()
    }

    fn connect(behaviour: &mut Behaviour, peer_id: PeerId, connection_id: ConnectionId) -> Handler {
        behaviour
            .handle_established_outbound_connection(
                connection_id,
                peer_id,
                &address(),
                Endpoint::Dialer,
                PortUse::Reuse,
            )
            .unwrap()
    }

    fn outbound_failures(events: &[ToSwarm<Event, FromBehaviourEvent>]) -> Vec<(StreamId, &StreamProtocol, &Error)> {
        events
            .iter()
            .filter_map(|event| match event {
                ToSwarm::GenerateEvent(Event::OutboundFailure {
                    stream_id,
                    protocol,
                    error,
                    ..
                }) => Some((*stream_id, protocol, error)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn connection_close_fails_pending_outbound_streams() {
        let peer_id = PeerId::random();
        let protocol = StreamProtocol::new("/test/1");
        let connection_id = ConnectionId::new_unchecked(1);
        let mut behaviour = Behaviour::new([protocol.clone()], Config::default());
        let _handler = connect(&mut behaviour, peer_id, connection_id);
        let stream_id = behaviour.open_substream(peer_id, protocol.clone());
        drain(&mut behaviour);

        let endpoint = ConnectedPoint::Dialer {
            address: address(),
            role_override: Endpoint::Dialer,
            port_use: PortUse::Reuse,
        };
        behaviour.on_swarm_event(FromSwarm::ConnectionClosed(ConnectionClosed {
            peer_id,
            connection_id,
            endpoint: &endpoint,
            cause: None,
            remaining_established: 0,
        }));

        let events = drain(&mut behaviour);
        assert_eq!(events.len(), 1);
        match outbound_failures(&events).as_slice() {
            [(failed_id, failed_protocol, Error::ConnectionClosed)] => {
                assert_eq!(*failed_id, stream_id);
                assert_eq!(**failed_protocol, protocol);
            },
            other => panic!("expected one connection-closed outbound failure, got {other:?}"),
        }
    }

    #[test]
    fn requests_to_an_unconnected_peer_share_one_dial() {
        let peer_id = PeerId::random();
        let protocol = StreamProtocol::new("/test/1");
        let mut behaviour = Behaviour::new([protocol.clone()], Config::default());
        let first = behaviour.open_substream(peer_id, protocol.clone());
        let second = behaviour.open_substream(peer_id, protocol.clone());

        let events = drain(&mut behaviour);
        let dials = events.iter().filter(|e| matches!(e, ToSwarm::Dial { .. })).count();
        assert_eq!(dials, 1);

        behaviour.on_swarm_event(FromSwarm::DialFailure(DialFailure {
            peer_id: Some(peer_id),
            error: &DialError::Aborted,
            connection_id: ConnectionId::new_unchecked(1),
        }));
        let events = drain(&mut behaviour);
        let failed = outbound_failures(&events)
            .into_iter()
            .map(|(stream_id, _, _)| stream_id)
            .collect::<Vec<_>>();
        assert_eq!(failed, [first, second]);
    }

    #[test]
    fn established_connection_takes_all_requests_queued_behind_one_dial() {
        let peer_id = PeerId::random();
        let protocol = StreamProtocol::new("/test/1");
        let mut behaviour = Behaviour::new([protocol.clone()], Config::default());
        behaviour.open_substream(peer_id, protocol.clone());
        behaviour.open_substream(peer_id, protocol.clone());
        drain(&mut behaviour);

        let mut handler = connect(&mut behaviour, peer_id, ConnectionId::new_unchecked(1));
        let mut cx = Context::from_waker(Waker::noop());
        for _ in 0..2 {
            assert!(matches!(
                handler.poll(&mut cx),
                Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { .. })
            ));
        }
        assert!(handler.poll(&mut cx).is_pending());

        // The next request to the peer reuses the connection.
        behaviour.open_substream(peer_id, protocol);
        let events = drain(&mut behaviour);
        assert!(matches!(events.as_slice(), [ToSwarm::NotifyHandler { .. }]));
    }

    #[test]
    fn registered_protocols_are_not_added_again() {
        let peer_id = PeerId::random();
        let protocol = StreamProtocol::new("/test/1");
        let mut behaviour = Behaviour::new([protocol.clone(), protocol.clone()], Config::default());
        assert_eq!(behaviour.supported_protocols(), slice::from_ref(&protocol));

        let _handler = connect(&mut behaviour, peer_id, ConnectionId::new_unchecked(1));
        behaviour.add_protocol(protocol.clone());
        assert_eq!(behaviour.supported_protocols(), [protocol]);
        assert!(drain(&mut behaviour).is_empty());
    }
}
