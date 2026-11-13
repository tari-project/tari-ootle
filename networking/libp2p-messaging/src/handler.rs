//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::VecDeque,
    convert::Infallible,
    future::{Future, Ready, ready},
    io,
    pin::pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_timer::Delay;
use libp2p::{
    InboundUpgrade,
    OutboundUpgrade,
    PeerId,
    Stream,
    StreamProtocol,
    core::UpgradeInfo,
    futures::{
        AsyncBufReadExt,
        AsyncRead,
        AsyncReadExt,
        AsyncWrite,
        AsyncWriteExt,
        FutureExt,
        SinkExt,
        StreamExt,
        channel::mpsc,
        future,
        future::Either,
        io::BufReader,
    },
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
    MessageId,
    codec::Codec,
    error::Error,
    event::Event,
    stream::{MessageStream, StreamId},
};

pub struct Handler<TCodec: Codec> {
    peer_id: PeerId,
    protocol: StreamProtocol,
    /// Streams with an outbound substream request in flight. libp2p reports one negotiation result per request.
    requested_streams: VecDeque<MessageStream<TCodec::Message>>,
    /// Streams from the behaviour awaiting an outbound substream request. The behaviour sends a connection one stream
    /// at a time; the queue keeps a later stream from replacing an earlier one regardless.
    pending_streams: VecDeque<MessageStream<TCodec::Message>>,
    pending_events: VecDeque<Event<TCodec::Message>>,
    pending_close_events: VecDeque<Event<TCodec::Message>>,
    pending_events_sender: mpsc::Sender<Event<TCodec::Message>>,
    pending_events_receiver: mpsc::Receiver<Event<TCodec::Message>>,
    codec: TCodec,
    timeouts: StreamTimeouts,
    inbound_tasks: futures_bounded::FuturesSet<Event<TCodec::Message>>,
    outbound_tasks: futures_bounded::FuturesSet<Event<TCodec::Message>>,
}

const TASK_TIMEOUT: Duration = Duration::from_secs(10000 * 24 * 60 * 60);

#[derive(Debug, Clone, Copy)]
struct StreamTimeouts {
    send_recv: Duration,
    inbound_idle: Duration,
    outbound_idle: Duration,
}

impl<TCodec: Codec> Handler<TCodec> {
    pub fn new(peer_id: PeerId, protocol: StreamProtocol, config: &Config) -> Self {
        let (pending_events_sender, pending_events_receiver) = mpsc::channel(config.inbound_message_buffer_size);
        Self {
            peer_id,
            protocol,
            requested_streams: VecDeque::new(),
            pending_streams: VecDeque::new(),
            pending_events: VecDeque::new(),
            pending_close_events: VecDeque::new(),
            codec: TCodec::default(),
            pending_events_sender,
            pending_events_receiver,
            timeouts: StreamTimeouts {
                send_recv: config.send_recv_timeout,
                inbound_idle: config.inbound_idle_timeout,
                outbound_idle: config.outbound_idle_timeout,
            },
            // Streams enforce their own per-message and idle deadlines. The task sets only bound concurrency, and are
            // separate so that inbound streams cannot take the capacity needed to send.
            inbound_tasks: futures_bounded::FuturesSet::new(TASK_TIMEOUT, config.max_concurrent_streams_per_peer),
            outbound_tasks: futures_bounded::FuturesSet::new(TASK_TIMEOUT, config.max_concurrent_streams_per_peer),
        }
    }
}

impl<TCodec> Handler<TCodec>
where TCodec: Codec + Send + Clone + 'static
{
    fn on_listen_upgrade_error(&self, error: ListenUpgradeError<(), Protocol<StreamProtocol>>) {
        tracing::warn!("unexpected listen upgrade error: {:?}", error.error);
    }

    /// Removes the request for `stream_id`. Outbound negotiations complete in any order, so the request is found by id
    /// rather than by position.
    fn take_requested_stream(&mut self, stream_id: StreamId) -> Option<MessageStream<TCodec::Message>> {
        let pos = self.requested_streams.iter().position(|s| s.stream_id() == stream_id)?;
        self.requested_streams.remove(pos)
    }

    fn on_dial_upgrade_error(&mut self, error: DialUpgradeError<StreamId, Protocol<StreamProtocol>>) {
        let stream = self.take_requested_stream(error.info);
        debug_assert!(
            stream.is_some(),
            "outbound stream {} upgrade failed with no requested stream",
            error.info
        );
        let Some(stream) = stream else {
            tracing::warn!("outbound stream {} upgrade failed with no requested stream", error.info);
            return;
        };

        match error.error {
            StreamUpgradeError::Timeout => {
                self.pending_events.push_back(Event::OutboundFailure {
                    peer_id: self.peer_id,
                    stream_id: stream.stream_id(),
                    error: Error::DialUpgradeError,
                });
            },
            StreamUpgradeError::NegotiationFailed => {
                // The remote merely doesn't support the protocol(s) we requested.
                // This is no reason to close the connection, which may
                // successfully communicate with other protocols already.
                // An event is reported to permit user code to react to the fact that
                // the remote peer does not support the requested protocol(s).
                self.pending_events.push_back(Event::OutboundFailure {
                    peer_id: self.peer_id,
                    stream_id: stream.stream_id(),
                    error: Error::ProtocolNotSupported,
                });
            },
            StreamUpgradeError::Apply(_) => {},
            StreamUpgradeError::Io(e) => {
                tracing::debug!("outbound stream {} failed to negotiate: {e}", stream.stream_id());
                // Dropping the stream closes its sink, so the behaviour opens a new stream for later messages.
                self.pending_events.push_back(Event::OutboundFailure {
                    peer_id: self.peer_id,
                    stream_id: stream.stream_id(),
                    error: Error::ProtocolNegotiationFailed(e),
                });
            },
        }
    }

    fn on_fully_negotiated_outbound(&mut self, outbound: FullyNegotiatedOutbound<Protocol<StreamProtocol>, StreamId>) {
        let (peer_stream, _protocol) = outbound.protocol;
        self.handle_negotiated_outbound(peer_stream, outbound.info);
    }

    fn handle_negotiated_outbound<TStream>(&mut self, peer_stream: TStream, stream_id: StreamId)
    where TStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {
        let codec = self.codec.clone();

        let msg_stream = self.take_requested_stream(stream_id);
        debug_assert!(
            msg_stream.is_some(),
            "negotiated outbound stream {stream_id} without a requested stream"
        );
        let Some(msg_stream) = msg_stream else {
            tracing::warn!("negotiated outbound stream {stream_id} without a requested stream");
            return;
        };

        let stream_id = msg_stream.stream_id();
        let events = self.pending_events_sender.clone();
        let fut = outbound_loop(codec, peer_stream, msg_stream, events, self.timeouts).boxed();

        if self.outbound_tasks.try_push(fut).is_err() {
            tracing::warn!("Dropping outbound stream because we are at capacity");
            self.pending_events.push_back(Event::OutboundFailure {
                peer_id: self.peer_id,
                stream_id,
                error: Error::ChannelClosed,
            });
            return;
        }

        self.pending_events.push_back(Event::OutboundStreamOpened {
            peer_id: self.peer_id,
            stream_id,
        });
    }

    fn on_fully_negotiated_inbound(&mut self, inbound: FullyNegotiatedInbound<Protocol<StreamProtocol>, ()>) {
        let (stream, _protocol) = inbound.protocol;
        self.handle_negotiated_inbound(stream);
    }

    fn handle_negotiated_inbound<TStream>(&mut self, stream: TStream)
    where TStream: AsyncRead + Unpin + Send + 'static {
        let codec = self.codec.clone();
        let peer_id = self.peer_id;
        let events = self.pending_events_sender.clone();
        let fut = inbound_loop(codec, stream, peer_id, events, self.timeouts).boxed();

        if self.inbound_tasks.try_push(fut).is_err() {
            tracing::warn!("Dropping inbound stream because we are at capacity");
            return;
        }

        self.pending_events.push_back(Event::InboundStreamOpened { peer_id });
    }
}

enum OutboundWait<TMsg> {
    Message(TMsg),
    Drained,
    RemoteClosed,
    Idle,
}

async fn outbound_loop<TCodec, TStream>(
    codec: TCodec,
    mut peer_stream: TStream,
    mut msg_stream: MessageStream<TCodec::Message>,
    mut events: mpsc::Sender<Event<TCodec::Message>>,
    timeouts: StreamTimeouts,
) -> Event<TCodec::Message>
where
    TCodec: Codec,
    TStream: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut message_id = MessageId::default();
    let stream_id = msg_stream.stream_id();
    let peer_id = *msg_stream.peer_id();
    loop {
        // The peer never writes to this stream, so a completed read means it has closed or reset it.
        let mut read_buf = [0u8; 1];
        let next = {
            let recv = pin!(msg_stream.recv());
            let remote_closed = peer_stream.read(&mut read_buf);
            let idle = Delay::new(timeouts.outbound_idle);
            match future::select(recv, future::select(remote_closed, idle)).await {
                Either::Left((Some(msg), _)) => OutboundWait::Message(msg),
                Either::Left((None, _)) => OutboundWait::Drained,
                Either::Right((Either::Left(_), _)) => OutboundWait::RemoteClosed,
                Either::Right((Either::Right(_), _)) => OutboundWait::Idle,
            }
        };
        let msg = match next {
            OutboundWait::Message(msg) => msg,
            // Closing the channel makes the behaviour open a new stream for later messages, while messages already
            // queued are still written here before the stream is closed.
            OutboundWait::Idle => {
                msg_stream.close();
                continue;
            },
            OutboundWait::Drained => {
                let _ignore = with_timeout(timeouts.send_recv, peer_stream.close()).await;
                break Event::StreamClosed { peer_id, stream_id };
            },
            OutboundWait::RemoteClosed => break Event::StreamClosed { peer_id, stream_id },
        };

        match with_timeout(timeouts.send_recv, async {
            codec.encode_to(&mut peer_stream, msg).await?;
            peer_stream.flush().await
        })
        .await
        {
            None => {
                break Event::OutboundFailure {
                    peer_id,
                    stream_id,
                    error: Error::SendTimeout(timeouts.send_recv),
                };
            },
            Some(Ok(())) => {
                if events.send(Event::MessageSent { message_id, stream_id }).await.is_err() {
                    // This should never happen. If the handler is dropped, the tasks are dropped too and
                    // therefore cannot be polled.
                    tracing::warn!("BUG: Internal event mpsc channel closed, closing outbound stream");
                    break Event::StreamClosed { peer_id, stream_id };
                }
            },
            Some(Err(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                break Event::StreamClosed { peer_id, stream_id };
            },
            Some(Err(e)) => {
                break Event::OutboundFailure {
                    peer_id,
                    stream_id,
                    error: Error::CodecError(e),
                };
            },
        }
        message_id = message_id.wrapping_add(1);
    }
}

async fn inbound_loop<TCodec, TStream>(
    codec: TCodec,
    stream: TStream,
    peer_id: PeerId,
    mut events: mpsc::Sender<Event<TCodec::Message>>,
    timeouts: StreamTimeouts,
) -> Event<TCodec::Message>
where
    TCodec: Codec,
    TStream: AsyncRead + Unpin + Send,
{
    // Buffered so that the first byte of the next message can be awaited without consuming it. Waiting for a message
    // to start is bounded by the idle timeout, and receiving it once started by the send/recv timeout.
    let mut stream = BufReader::new(stream);
    loop {
        match with_timeout(timeouts.inbound_idle, stream.fill_buf()).await {
            None => break Event::InboundStreamClosed { peer_id },
            Some(Ok([])) => break Event::InboundStreamClosed { peer_id },
            Some(Ok(_)) => {},
            Some(Err(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                break Event::InboundStreamClosed { peer_id };
            },
            Some(Err(e)) => break Event::Error(Error::CodecError(e)),
        }

        let Some(result) = with_timeout(timeouts.send_recv, codec.decode_from(&mut stream)).await else {
            break Event::Error(Error::ReceiveTimeout(timeouts.send_recv));
        };

        match result {
            Ok((length, msg)) => {
                if events
                    .send(Event::ReceivedMessage {
                        peer_id,
                        message: msg,
                        length,
                    })
                    .await
                    .is_err()
                {
                    tracing::warn!("BUG: Internal event mpsc channel closed, closing inbound stream");
                    break Event::InboundStreamClosed { peer_id };
                }
            },
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                break Event::InboundStreamClosed { peer_id };
            },
            Err(e) => {
                break Event::Error(Error::CodecError(e));
            },
        }
    }
}

async fn with_timeout<F: Future>(timeout: Duration, fut: F) -> Option<F::Output> {
    match future::select(pin!(fut), Delay::new(timeout)).await {
        Either::Left((output, _)) => Some(output),
        Either::Right(_) => None,
    }
}

impl<TCodec> ConnectionHandler for Handler<TCodec>
where TCodec: Codec + Send + Clone + 'static
{
    type FromBehaviour = MessageStream<TCodec::Message>;
    type InboundOpenInfo = ();
    type InboundProtocol = Protocol<StreamProtocol>;
    type OutboundOpenInfo = StreamId;
    type OutboundProtocol = Protocol<StreamProtocol>;
    type ToBehaviour = Event<TCodec::Message>;

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
        // Drain pending events that were produced by handler (such as stream-opened notifications) before polling
        // stream tasks so that a stream is always reported opened before any message or close event from its task.
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }
        if self.pending_events.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_events.shrink_to_fit();
        }

        // Drain any message events already queued by stream tasks before polling tasks again or emitting close events.
        if let Poll::Ready(Some(event)) = self.pending_events_receiver.poll_next_unpin(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }

        while let Poll::Ready(result) = self.outbound_tasks.poll_unpin(cx) {
            match result {
                Ok(event @ Event::OutboundFailure { peer_id, stream_id, .. }) => {
                    self.pending_close_events.push_back(event);
                    self.pending_close_events
                        .push_back(Event::StreamClosed { peer_id, stream_id });
                },
                Ok(event) => self.pending_close_events.push_back(event),
                Err(err) => self.pending_close_events.push_back(Event::Error(Error::Timeout(err))),
            }
        }
        while let Poll::Ready(result) = self.inbound_tasks.poll_unpin(cx) {
            match result {
                Ok(event @ Event::InboundStreamClosed { .. }) => self.pending_close_events.push_back(event),
                Ok(event) => {
                    self.pending_close_events.push_back(event);
                    self.pending_close_events.push_back(Event::InboundStreamClosed {
                        peer_id: self.peer_id,
                    });
                },
                Err(err) => {
                    self.pending_close_events.push_back(Event::Error(Error::Timeout(err)));
                    self.pending_close_events.push_back(Event::InboundStreamClosed {
                        peer_id: self.peer_id,
                    });
                },
            }
        }

        // Emit message events produced by stream tasks during this poll before any task completion events.
        if let Poll::Ready(Some(event)) = self.pending_events_receiver.poll_next_unpin(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }

        if let Some(event) = self.pending_close_events.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }
        if self.pending_close_events.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_close_events.shrink_to_fit();
        }

        // Open outbound stream.
        if let Some(stream) = self.pending_streams.pop_front() {
            let stream_id = stream.stream_id();
            self.requested_streams.push_back(stream);

            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    Protocol {
                        protocol: self.protocol.clone(),
                    },
                    stream_id,
                ),
            });
        }

        Poll::Pending
    }

    fn on_behaviour_event(&mut self, stream: Self::FromBehaviour) {
        self.pending_streams.push_back(stream);
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

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        future::Future,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use libp2p::futures::{AsyncReadExt, AsyncWriteExt, executor::block_on, future, future::Either};

    use super::*;
    use crate::stream;

    const GUARD: Duration = Duration::from_secs(5);

    #[derive(Debug, Clone, Default)]
    pub(crate) struct TestCodec;

    #[async_trait::async_trait]
    impl Codec for TestCodec {
        type Message = Vec<u8>;

        async fn decode_from<R>(&self, reader: &mut R) -> io::Result<(usize, Self::Message)>
        where R: AsyncRead + Unpin + Send {
            let mut len_buf = [0u8; 4];
            reader.read_exact(&mut len_buf).await?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf).await?;
            Ok((len, buf))
        }

        async fn encode_to<W>(&self, writer: &mut W, message: Self::Message) -> io::Result<()>
        where W: AsyncWrite + Unpin + Send {
            writer.write_all(&(message.len() as u32).to_be_bytes()).await?;
            writer.write_all(&message).await
        }
    }

    /// Yields `data`, then either reports EOF or stays pending forever. Writes are either accepted, fail, or stay
    /// pending forever.
    struct TestStream {
        data: Vec<u8>,
        pos: usize,
        eof_after_data: bool,
        accept_writes: bool,
        write_error: Option<io::ErrorKind>,
        /// Bytes written before the stream was closed.
        written: Arc<AtomicUsize>,
        closed: Arc<AtomicBool>,
    }

    impl TestStream {
        fn stalled() -> Self {
            Self::stalled_after(vec![])
        }

        fn stalled_after(data: Vec<u8>) -> Self {
            Self {
                data,
                pos: 0,
                eof_after_data: false,
                accept_writes: false,
                write_error: None,
                written: Arc::default(),
                closed: Arc::default(),
            }
        }

        fn closed_after(data: Vec<u8>) -> Self {
            Self {
                data,
                pos: 0,
                eof_after_data: true,
                accept_writes: true,
                write_error: None,
                written: Arc::default(),
                closed: Arc::default(),
            }
        }

        /// Accepts writes and never yields data or EOF, like a healthy peer receiving on this stream.
        fn open() -> Self {
            Self {
                data: vec![],
                pos: 0,
                eof_after_data: false,
                accept_writes: true,
                write_error: None,
                written: Arc::default(),
                closed: Arc::default(),
            }
        }

        fn broken_write(kind: io::ErrorKind) -> Self {
            Self {
                data: vec![],
                pos: 0,
                eof_after_data: false,
                accept_writes: false,
                write_error: Some(kind),
                written: Arc::default(),
                closed: Arc::default(),
            }
        }
    }

    impl AsyncRead for TestStream {
        fn poll_read(mut self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
            let pos = self.pos;
            let remaining = &self.data[pos..];
            if remaining.is_empty() {
                return if self.eof_after_data {
                    Poll::Ready(Ok(0))
                } else {
                    Poll::Pending
                };
            }
            let n = remaining.len().min(buf.len());
            buf[..n].copy_from_slice(&remaining[..n]);
            self.pos += n;
            Poll::Ready(Ok(n))
        }
    }

    impl AsyncWrite for TestStream {
        fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
            if let Some(kind) = self.write_error {
                return Poll::Ready(Err(io::Error::from(kind)));
            }
            if self.accept_writes {
                if !self.closed.load(Ordering::SeqCst) {
                    self.written.fetch_add(buf.len(), Ordering::SeqCst);
                }
                Poll::Ready(Ok(buf.len()))
            } else {
                Poll::Pending
            }
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.closed.store(true, Ordering::SeqCst);
            Poll::Ready(Ok(()))
        }
    }

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut buf = (payload.len() as u32).to_be_bytes().to_vec();
        buf.extend_from_slice(payload);
        buf
    }

    fn timeouts(send_recv_ms: u64, idle_ms: u64) -> StreamTimeouts {
        StreamTimeouts {
            send_recv: Duration::from_millis(send_recv_ms),
            inbound_idle: Duration::from_millis(idle_ms),
            outbound_idle: Duration::from_millis(idle_ms),
        }
    }

    /// Runs `fut` to completion, or panics if it is still running after `GUARD`.
    fn run_guarded<F: Future>(fut: F) -> F::Output {
        let fut = std::pin::pin!(fut);
        match block_on(future::select(fut, futures_timer::Delay::new(GUARD))) {
            Either::Left((output, _)) => output,
            Either::Right(_) => panic!("stream task still running after {GUARD:?}"),
        }
    }

    #[test]
    fn inbound_stream_that_never_sends_is_closed() {
        let peer_id = PeerId::random();
        let (events, _rx) = mpsc::channel(10);
        let event = run_guarded(inbound_loop(
            TestCodec,
            TestStream::stalled(),
            peer_id,
            events,
            timeouts(100, 100),
        ));
        assert!(matches!(event, Event::InboundStreamClosed { peer_id: p } if p == peer_id));
    }

    #[test]
    fn inbound_message_that_stalls_part_way_times_out() {
        let mut partial = frame(&[1u8; 64]);
        partial.truncate(10);
        let (events, _rx) = mpsc::channel(10);
        let event = run_guarded(inbound_loop(
            TestCodec,
            TestStream::stalled_after(partial),
            PeerId::random(),
            events,
            timeouts(100, 60_000),
        ));
        assert!(matches!(event, Event::Error(Error::ReceiveTimeout(_))), "{event:?}");
    }

    #[test]
    fn inbound_messages_are_delivered() {
        let mut data = frame(b"hello");
        data.extend(frame(b"world"));
        let (events, mut rx) = mpsc::channel(10);
        let event = run_guarded(inbound_loop(
            TestCodec,
            TestStream::closed_after(data),
            PeerId::random(),
            events,
            timeouts(100, 100),
        ));
        assert!(matches!(event, Event::InboundStreamClosed { .. }), "{event:?}");

        let mut received = vec![];
        while let Ok(event) = rx.try_recv() {
            if let Event::ReceivedMessage { message, .. } = event {
                received.push(message);
            }
        }
        assert_eq!(received, vec![b"hello".to_vec(), b"world".to_vec()]);
    }

    #[test]
    fn outbound_message_the_peer_does_not_accept_times_out() {
        let (mut sink, msg_stream) = stream::channel(1, PeerId::random());
        sink.send(b"hello".to_vec()).unwrap();
        let (events, _rx) = mpsc::channel(10);
        let event = run_guarded(outbound_loop(
            TestCodec,
            TestStream::stalled(),
            msg_stream,
            events,
            timeouts(100, 100),
        ));
        assert!(
            matches!(event, Event::OutboundFailure {
                stream_id: 1,
                error: Error::SendTimeout(_),
                ..
            }),
            "{event:?}"
        );
    }

    #[test]
    fn outbound_codec_write_error_reports_outbound_failure_for_stream() {
        let peer_id = PeerId::random();
        let (mut sink, msg_stream) = stream::channel(42, peer_id);
        sink.send(b"hello".to_vec()).unwrap();
        let (events, _rx) = mpsc::channel(10);
        let event = run_guarded(outbound_loop(
            TestCodec,
            TestStream::broken_write(io::ErrorKind::BrokenPipe),
            msg_stream,
            events,
            timeouts(100, 100),
        ));
        assert!(
            matches!(
                event,
                Event::OutboundFailure {
                    peer_id: p,
                    stream_id: 42,
                    error: Error::CodecError(ref e),
                } if p == peer_id && e.kind() == io::ErrorKind::BrokenPipe
            ),
            "{event:?}"
        );
    }

    #[test]
    fn idle_outbound_stream_is_closed_by_the_sender() {
        let (sink, msg_stream) = stream::channel(1, PeerId::random());
        let peer_stream = TestStream::open();
        let closed = peer_stream.closed.clone();
        let (events, _rx) = mpsc::channel(10);
        let event = run_guarded(outbound_loop(
            TestCodec,
            peer_stream,
            msg_stream,
            events,
            StreamTimeouts {
                send_recv: Duration::from_secs(60),
                inbound_idle: Duration::from_secs(60),
                outbound_idle: Duration::from_millis(100),
            },
        ));
        assert!(matches!(event, Event::StreamClosed { stream_id: 1, .. }), "{event:?}");
        assert!(
            closed.load(Ordering::SeqCst),
            "expected the stream to be closed gracefully"
        );
        assert!(sink.is_closed(), "expected the sink to stop accepting messages");
    }

    #[test]
    fn queued_messages_are_written_before_a_closing_stream_is_closed() {
        let (mut sink, mut msg_stream) = stream::channel(1, PeerId::random());
        sink.send(b"hello".to_vec()).unwrap();
        msg_stream.close();
        let peer_stream = TestStream::open();
        let written = peer_stream.written.clone();
        let closed = peer_stream.closed.clone();
        let (events, _rx) = mpsc::channel(10);
        let event = run_guarded(outbound_loop(
            TestCodec,
            peer_stream,
            msg_stream,
            events,
            timeouts(60_000, 60_000),
        ));
        assert!(matches!(event, Event::StreamClosed { stream_id: 1, .. }), "{event:?}");
        assert_eq!(written.load(Ordering::SeqCst), frame(b"hello").len());
        assert!(closed.load(Ordering::SeqCst));
    }

    #[test]
    fn outbound_stream_ends_when_the_peer_closes_it() {
        let (_sink, msg_stream) = stream::channel(1, PeerId::random());
        let (events, _rx) = mpsc::channel(10);
        let event = run_guarded(outbound_loop(
            TestCodec,
            TestStream::closed_after(vec![]),
            msg_stream,
            events,
            timeouts(100, 100),
        ));
        assert!(matches!(event, Event::StreamClosed { stream_id: 1, .. }), "{event:?}");
    }

    #[test]
    fn outbound_negotiation_io_error_is_reported_and_releases_the_stream() {
        let peer_id = PeerId::random();
        let mut handler = Handler::<TestCodec>::new(peer_id, StreamProtocol::new("/test/1"), &Config::default());
        let (mut sink, msg_stream) = stream::channel(7, peer_id);
        handler.on_behaviour_event(msg_stream);

        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol }) if *protocol.info() == 7
        ));

        handler.on_dial_upgrade_error(DialUpgradeError {
            info: 7,
            error: StreamUpgradeError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid multistream message",
            )),
        });

        match handler.poll(&mut cx) {
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::OutboundFailure {
                stream_id: 7,
                error: Error::ProtocolNegotiationFailed(_),
                ..
            })) => {},
            other => panic!("expected an outbound failure, got {:?}", other.map(|_| ())),
        }
        assert!(
            sink.is_closed(),
            "the handler must not keep the failed stream's receiver"
        );
        assert!(sink.send(b"later".to_vec()).is_err());
        assert!(handler.poll(&mut cx).is_pending());
    }

    #[test]
    fn multiple_pending_outbound_streams_are_queued_in_order() {
        let peer_id = PeerId::random();
        let mut handler = Handler::<TestCodec>::new(peer_id, StreamProtocol::new("/test/1"), &Config::default());
        let (_sink1, stream1) = stream::channel(1, peer_id);
        let (_sink2, stream2) = stream::channel(2, peer_id);
        handler.on_behaviour_event(stream1);
        handler.on_behaviour_event(stream2);

        let mut cx = Context::from_waker(std::task::Waker::noop());
        for expected_id in [1, 2] {
            assert!(matches!(
                handler.poll(&mut cx),
                Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol })
                    if *protocol.info() == expected_id
            ));
        }

        for expected_id in [1, 2] {
            handler.on_dial_upgrade_error(DialUpgradeError {
                info: expected_id,
                error: StreamUpgradeError::NegotiationFailed,
            });
            match handler.poll(&mut cx) {
                Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::OutboundFailure {
                    stream_id,
                    error: Error::ProtocolNotSupported,
                    ..
                })) => assert_eq!(stream_id, expected_id),
                other => panic!("expected an outbound failure, got {:?}", other.map(|_| ())),
            }
        }
    }

    #[test]
    fn out_of_order_outbound_negotiation_matches_requests_by_stream_id() {
        let peer_id = PeerId::random();
        let mut handler = Handler::<TestCodec>::new(peer_id, StreamProtocol::new("/test/1"), &Config::default());
        let (mut sink1, stream1) = stream::channel(1, peer_id);
        let (sink2, stream2) = stream::channel(2, peer_id);
        sink1.send(b"for-stream-1".to_vec()).unwrap();
        handler.on_behaviour_event(stream1);
        handler.on_behaviour_event(stream2);

        let mut cx = Context::from_waker(std::task::Waker::noop());
        for expected_id in [1, 2] {
            assert!(matches!(
                handler.poll(&mut cx),
                Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol })
                    if *protocol.info() == expected_id
            ));
        }

        // Stream 2 fails negotiation first while stream 1 is still in flight.
        handler.on_dial_upgrade_error(DialUpgradeError {
            info: 2,
            error: StreamUpgradeError::NegotiationFailed,
        });
        assert!(sink2.is_closed(), "stream 2 sink should be closed upon failure");
        assert!(!sink1.is_closed(), "stream 1 sink must remain open while negotiating");

        match handler.poll(&mut cx) {
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::OutboundFailure {
                stream_id: 2,
                error: Error::ProtocolNotSupported,
                ..
            })) => {},
            other => panic!("expected OutboundFailure for stream 2, got {:?}", other.map(|_| ())),
        }

        // Now stream 1 completes negotiation and delivers its queued message before closing.
        drop(sink1);
        let peer_stream = TestStream::open();
        let written = peer_stream.written.clone();
        handler.handle_negotiated_outbound(peer_stream, 1);

        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::OutboundStreamOpened {
                stream_id: 1,
                ..
            }))
        ));
        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::MessageSent {
                message_id: 0,
                stream_id: 1,
            }))
        ));
        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::StreamClosed {
                stream_id: 1,
                ..
            }))
        ));
        assert_eq!(written.load(Ordering::SeqCst), frame(b"for-stream-1").len());
        assert!(handler.poll(&mut cx).is_pending());
    }

    #[test]
    fn inbound_stream_events_are_emitted_in_causal_order() {
        let peer_id = PeerId::random();
        let mut handler = Handler::<TestCodec>::new(peer_id, StreamProtocol::new("/test/1"), &Config::default());
        let mut data = frame(b"first");
        data.extend(frame(b"second"));
        handler.handle_negotiated_inbound(TestStream::closed_after(data));

        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::InboundStreamOpened {
                peer_id: p,
            })) if p == peer_id
        ));
        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::ReceivedMessage {
                peer_id: p,
                ref message,
                ..
            })) if p == peer_id && message == b"first"
        ));
        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::ReceivedMessage {
                peer_id: p,
                ref message,
                ..
            })) if p == peer_id && message == b"second"
        ));
        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::InboundStreamClosed {
                peer_id: p,
            })) if p == peer_id
        ));
        assert!(handler.poll(&mut cx).is_pending());
    }

    #[test]
    fn failed_streams_emit_close_events_and_capacity_overflow_emits_outbound_failure() {
        let peer_id = PeerId::random();
        let config = Config {
            max_concurrent_streams_per_peer: 1,
            ..Config::default()
        };
        let mut handler = Handler::<TestCodec>::new(peer_id, StreamProtocol::new("/test/1"), &config);
        let mut cx = Context::from_waker(std::task::Waker::noop());

        let (mut sink1, stream1) = stream::channel(10, peer_id);
        let (sink2, stream2) = stream::channel(11, peer_id);
        sink1.send(b"boom".to_vec()).unwrap();
        handler.on_behaviour_event(stream1);
        handler.on_behaviour_event(stream2);

        for expected_id in [10, 11] {
            assert!(matches!(
                handler.poll(&mut cx),
                Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol })
                    if *protocol.info() == expected_id
            ));
        }

        // Stream 10 opens and then fails on write; Stream 11 exceeds max_concurrent_streams_per_peer (1).
        handler.handle_negotiated_outbound(TestStream::broken_write(io::ErrorKind::BrokenPipe), 10);
        handler.handle_negotiated_outbound(TestStream::open(), 11);
        assert!(sink2.is_closed());

        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::OutboundStreamOpened {
                stream_id: 10,
                ..
            }))
        ));
        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::OutboundFailure {
                stream_id: 11,
                error: Error::ChannelClosed,
                ..
            }))
        ));
        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::OutboundFailure {
                stream_id: 10,
                error: Error::CodecError(_),
                ..
            }))
        ));
        assert!(matches!(
            handler.poll(&mut cx),
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::StreamClosed {
                stream_id: 10,
                ..
            }))
        ));
        assert!(handler.poll(&mut cx).is_pending());
    }
}
