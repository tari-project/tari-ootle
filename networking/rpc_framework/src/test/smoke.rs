//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! End-to-end tests over a real [`RpcServer`] driven across a duplex stream, one substream per
//! session, in place of a transport.

use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use libp2p::{PeerId, StreamProtocol};
use libp2p_substream::{ProtocolEvent, ProtocolNotification};
use tari_shutdown::Shutdown;
use tokio::{
    io::DuplexStream,
    sync::{RwLock, mpsc},
    task,
    time,
};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

use crate::{
    NamedProtocolService,
    RPC_MAX_FRAME_SIZE,
    RpcClient,
    RpcError,
    RpcHandshakeError,
    RpcServer,
    RpcServerBuilder,
    RpcStatusCode,
    error::HandshakeRejectReason,
    framing,
    framing::CanonicalFraming,
    max_response_payload_size,
    test::greeting_service::{
        GreetingClient,
        GreetingRpc,
        GreetingServer,
        GreetingService,
        SayHelloRequest,
        SlowGreetingService,
        SlowStreamRequest,
    },
};

type TestSubstream = Compat<DuplexStream>;

/// Stands in for a transport's flow-control window: a server streaming to a client that has stopped
/// reading fills this and then blocks, as it would on a real connection.
const TRANSPORT_WINDOW: usize = 64 * 1024;

/// An `RpcServer` fed substreams directly, as the connection manager would feed it substreams from a
/// transport.
struct TestRpcServer {
    notif_tx: mpsc::UnboundedSender<ProtocolNotification<TestSubstream>>,
    shutdown: Shutdown,
    handle: task::JoinHandle<()>,
}

impl TestRpcServer {
    fn spawn<T: GreetingRpc>(service: T, builder: RpcServerBuilder) -> Self {
        let (notif_tx, notif_rx) = mpsc::unbounded_channel();
        let shutdown = Shutdown::new();
        let handle = task::spawn({
            let shutdown_signal = shutdown.to_signal();
            async move {
                let fut = builder
                    .finish()
                    .add_service(GreetingServer::new(service))
                    .serve(notif_rx);

                tokio::select! {
                    biased;
                    _ = shutdown_signal => {},
                    r = fut => r.unwrap(),
                }
            }
        });

        Self {
            notif_tx,
            shutdown,
            handle,
        }
    }

    fn with_sessions<T: GreetingRpc>(service: T, num_sessions: usize) -> Self {
        Self::spawn(
            service,
            RpcServer::builder()
                .with_maximum_simultaneous_sessions(num_sessions)
                .with_minimum_client_deadline(Duration::from_secs(0)),
        )
    }

    /// Hands the server one end of a new substream as an inbound dial from `peer_id`, and returns
    /// the other end framed for a client.
    fn dial_as(&self, peer_id: PeerId, protocol: StreamProtocol, window: usize) -> CanonicalFraming<TestSubstream> {
        let (server_io, client_io) = tokio::io::duplex(window);
        self.notif_tx
            .send(ProtocolNotification::new(
                protocol,
                ProtocolEvent::NewInboundSubstream {
                    peer_id,
                    substream: server_io.compat(),
                },
            ))
            .unwrap();
        framing::canonical(client_io.compat(), RPC_MAX_FRAME_SIZE)
    }

    fn dial(&self) -> CanonicalFraming<TestSubstream> {
        self.dial_with_window(TRANSPORT_WINDOW)
    }

    fn dial_with_window(&self, window: usize) -> CanonicalFraming<TestSubstream> {
        self.dial_as(
            PeerId::random(),
            StreamProtocol::new(GreetingClient::PROTOCOL_NAME),
            window,
        )
    }

    fn dial_peer(&self, peer_id: PeerId) -> CanonicalFraming<TestSubstream> {
        self.dial_as(
            peer_id,
            StreamProtocol::new(GreetingClient::PROTOCOL_NAME),
            TRANSPORT_WINDOW,
        )
    }

    async fn shutdown(self) {
        self.shutdown.trigger();
        self.handle.await.unwrap();
    }
}

/// Connects a client over `framed`, with deadlines wide enough that a slow CI machine does not trip
/// them.
async fn connect(framed: CanonicalFraming<TestSubstream>) -> Result<GreetingClient, RpcError> {
    connect_with_deadline(framed, Duration::from_secs(5)).await
}

async fn connect_with_deadline(
    framed: CanonicalFraming<TestSubstream>,
    deadline: Duration,
) -> Result<GreetingClient, RpcError> {
    RpcClient::builder::<GreetingClient>(PeerId::random())
        .with_deadline(deadline)
        .with_deadline_grace_period(Duration::from_secs(1))
        .with_handshake_timeout(Duration::from_secs(5))
        .connect(framed)
        .await
}

/// The reason the server gave for refusing this session.
///
/// A refusal surfaces at whichever point the peer's close beats. The handshake does not wait for a
/// reply, so when the peer closes before the client's write lands the reason comes back from
/// connecting; otherwise it comes back from the first request, in place of its response.
async fn refusal_reason(framed: CanonicalFraming<TestSubstream>) -> String {
    match connect(framed).await {
        Err(RpcError::HandshakeError(RpcHandshakeError::Rejected(reason))) => reason.to_string(),
        Err(err) => panic!("expected the session to be refused, got {err:?}"),
        Ok(mut client) => {
            let err = client.say_hello(SayHelloRequest::default()).await.unwrap_err();
            let RpcError::RequestFailed(status) = err else {
                panic!("expected the session to be refused, got {err:?}");
            };
            assert_eq!(status.as_status_code(), RpcStatusCode::HandshakeDenied);
            status.details().to_string()
        },
    }
}

#[tokio::test]
async fn request_response_errors_and_streaming() {
    let server = TestRpcServer::with_sessions(GreetingService::default(), 1);
    let mut client = connect(server.dial()).await.unwrap();

    // Latency is available "for free" as part of the connect protocol
    assert!(client.get_last_request_latency().is_some());

    let resp = client
        .say_hello(SayHelloRequest {
            name: "Yathvan".to_string(),
            language: 1,
        })
        .await
        .unwrap();
    assert_eq!(resp.greeting, "Jambo Yathvan");

    let resp = client.get_greetings(4).await.unwrap();
    let greetings = resp.map(|r| r.unwrap()).collect::<Vec<_>>().await;
    assert_eq!(greetings, ["Sawubona", "Jambo", "Bonjour", "Hello"]);

    let err = client.return_error().await.unwrap_err();
    let RpcError::RequestFailed(status) = err else {
        panic!("expected a request failure, got {err:?}");
    };
    assert_eq!(status.as_status_code(), RpcStatusCode::NotImplemented);
    assert_eq!(status.details(), "I haven't gotten to this yet :(");

    let stream = client.streaming_error("Gurglesplurb".to_string()).await.unwrap();
    let status = stream
        // StreamExt::collect has a Default trait bound which Result<_, _> cannot satisfy, so the
        // results are collected into a Vec first.
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<String, _>>()
        .unwrap_err();
    assert_eq!(status.as_status_code(), RpcStatusCode::BadRequest);
    assert_eq!(status.details(), "What does 'Gurglesplurb' mean?");

    let stream = client.streaming_error2().await.unwrap();
    let results = stream.collect::<Vec<_>>().await;
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].as_ref().unwrap(), "This is ok");

    let second_reply = results[1].as_ref().unwrap_err();
    assert_eq!(second_reply.as_status_code(), RpcStatusCode::BadRequest);
    assert_eq!(second_reply.details(), "This is a problem");

    client.close().await;

    let err = client.say_hello(SayHelloRequest::default()).await.unwrap_err();
    assert!(
        // Closing the request stream races the send on it, so either answer is correct.
        matches!(err, RpcError::ClientClosed | RpcError::RequestCancelled),
        "unexpected error {err:?}"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn concurrent_requests() {
    let server = TestRpcServer::with_sessions(GreetingService::default(), 1);
    let mut client = connect(server.dial()).await.unwrap();

    let mut cloned_client = client.clone();
    let spawned1 = task::spawn(async move {
        cloned_client
            .say_hello(SayHelloRequest {
                name: "Madeupington".to_string(),
                language: 2,
            })
            .await
            .unwrap()
    });
    let mut cloned_client = client.clone();
    let spawned2 = task::spawn(async move {
        let resp = cloned_client.get_greetings(5).await.unwrap().collect::<Vec<_>>().await;
        resp.into_iter().map(Result::unwrap).collect::<Vec<_>>()
    });
    let resp = client
        .say_hello(SayHelloRequest {
            name: "Yathvan".to_string(),
            language: 1,
        })
        .await
        .unwrap();
    assert_eq!(resp.greeting, "Jambo Yathvan");

    assert_eq!(spawned1.await.unwrap().greeting, "Bonjour Madeupington");
    assert_eq!(spawned2.await.unwrap(), GreetingService::DEFAULT_GREETINGS[..5]);
}

#[tokio::test]
async fn response_too_big() {
    let server = TestRpcServer::with_sessions(GreetingService::new(&[]), 1);
    let mut client = connect(server.dial_with_window(RPC_MAX_FRAME_SIZE)).await.unwrap();

    // The response overhead means a payload of exactly the frame size is always too large.
    let err = client
        .reply_with_msg_of_size(max_response_payload_size() as u64 + 1)
        .await
        .unwrap_err();
    let RpcError::RequestFailed(status) = err else {
        panic!("expected a request failure, got {err:?}");
    };
    assert_eq!(status.as_status_code(), RpcStatusCode::MalformedResponse);

    // The exact frame size boundary works, and the session survives the rejection above.
    let reply = client
        .reply_with_msg_of_size(max_response_payload_size() as u64 - 9)
        .await
        .unwrap();
    assert_eq!(reply.len(), max_response_payload_size() - 9);
}

#[tokio::test]
async fn ping_latency() {
    let server = TestRpcServer::with_sessions(GreetingService::new(&[]), 1);
    let mut client = connect(server.dial()).await.unwrap();

    let latency = client.ping().await.unwrap();
    // Typically well under a millisecond over a duplex stream; the bound is wide for slow CI.
    assert!(latency.as_secs() < 5);
}

#[tokio::test]
async fn timeout() {
    let delay = Arc::new(RwLock::new(Duration::from_secs(10)));
    let server = TestRpcServer::with_sessions(SlowGreetingService::new(delay.clone()), 1);
    let mut client = connect_with_deadline(server.dial(), Duration::from_secs(1))
        .await
        .unwrap();

    let err = client.say_hello(SayHelloRequest::default()).await.unwrap_err();
    let RpcError::RequestFailed(status) = err else {
        panic!("expected a request failure, got {err:?}");
    };
    assert_eq!(status.as_status_code(), RpcStatusCode::Timeout);

    *delay.write().await = Duration::from_secs(0);

    // The server abandons the request at the deadline and waits for the next one rather than
    // ending the session, so the next request is answered normally.
    let resp = client.say_hello(SayHelloRequest::default()).await.unwrap();
    assert_eq!(resp.greeting, "took a while to load");
}

#[tokio::test]
async fn stream_still_works_after_cancel() {
    let service_impl = GreetingService::default();
    let server = TestRpcServer::with_sessions(service_impl.clone(), 1);
    let mut client = connect(server.dial()).await.unwrap();

    // Ask for a stream and immediately throw away the receiver.
    client
        .slow_stream(SlowStreamRequest {
            num_items: 100,
            item_size: 100,
            delay_ms: 10,
        })
        .await
        .unwrap();
    assert_eq!(service_impl.call_count(), 1);

    let resp = client
        .slow_stream(SlowStreamRequest {
            num_items: 100,
            item_size: 100,
            delay_ms: 10,
        })
        .await
        .unwrap();

    resp.collect::<Vec<_>>().await.into_iter().for_each(|r| {
        r.unwrap();
    });
}

#[tokio::test]
async fn a_session_recovers_from_an_abandoned_stream() {
    let service_impl = GreetingService::default();
    let server = TestRpcServer::with_sessions(service_impl.clone(), 1);
    let mut client = connect(server.dial()).await.unwrap();

    // Abandoned after one item, leaving the rest of the stream to arrive against a request that is
    // no longer waiting for it.
    let mut resp = client
        .slow_stream(SlowStreamRequest {
            num_items: 50,
            item_size: 100,
            delay_ms: 1,
        })
        .await
        .unwrap();
    let _buffer = resp.next().await.unwrap().unwrap();
    drop(resp);

    // The next request has to read past every frame the abandoned one left behind before its own
    // response, and there are more of them than a session may discard by any fixed count.
    let resp = client.get_greetings(4).await.unwrap();
    let greetings = resp.map(|r| r.unwrap()).collect::<Vec<_>>().await;
    assert_eq!(greetings, ["Sawubona", "Jambo", "Bonjour", "Hello"]);

    // And the session is still good for another after that.
    let resp = client
        .say_hello(SayHelloRequest {
            name: "Yathvan".to_string(),
            language: 1,
        })
        .await
        .unwrap();
    assert_eq!(resp.greeting, "Jambo Yathvan");
}

#[tokio::test]
async fn a_session_beyond_the_server_s_limit_is_refused() {
    // A server with no session slots refuses every session it is offered.
    let server = TestRpcServer::with_sessions(GreetingService::new(&[]), 0);
    assert_eq!(
        refusal_reason(server.dial()).await,
        HandshakeRejectReason::NoSessionsAvailable.to_string()
    );
}

#[tokio::test]
async fn a_session_for_an_unknown_protocol_is_refused() {
    let server = TestRpcServer::with_sessions(GreetingService::new(&[]), 1);
    let framed = server.dial_as(
        PeerId::random(),
        StreamProtocol::new("/test/this-is-junk/1.0"),
        TRANSPORT_WINDOW,
    );
    assert_eq!(
        refusal_reason(framed).await,
        HandshakeRejectReason::ProtocolNotSupported.to_string()
    );
}

#[tokio::test]
async fn max_global_sessions() {
    let server = TestRpcServer::spawn(
        GreetingService::default(),
        RpcServer::builder().with_maximum_simultaneous_sessions(1),
    );

    let mut first = connect(server.dial()).await.unwrap();
    first.say_hello(SayHelloRequest::default()).await.unwrap();

    assert_eq!(
        refusal_reason(server.dial()).await,
        HandshakeRejectReason::NoSessionsAvailable.to_string()
    );

    // Closing the first session frees its slot.
    first.close().await;
    wait_for_session(&server).await;
}

#[tokio::test]
async fn max_per_client_sessions() {
    let server = TestRpcServer::spawn(
        GreetingService::default(),
        RpcServer::builder()
            .with_maximum_simultaneous_sessions(3)
            .with_maximum_sessions_per_client(1),
    );
    let peer_id = PeerId::random();

    let mut first = connect(server.dial_peer(peer_id)).await.unwrap();
    first.say_hello(SayHelloRequest::default()).await.unwrap();

    assert_eq!(
        refusal_reason(server.dial_peer(peer_id)).await,
        HandshakeRejectReason::NoSessionsAvailable.to_string()
    );

    // Another peer is unaffected by the per-client limit.
    let mut other = connect(server.dial_peer(PeerId::random())).await.unwrap();
    other.say_hello(SayHelloRequest::default()).await.unwrap();
}

#[tokio::test]
async fn failed_handshakes_do_not_count_against_the_per_client_limit() {
    let server = TestRpcServer::spawn(
        GreetingService::default(),
        RpcServer::builder().with_maximum_sessions_per_client(2),
    );
    let peer_id = PeerId::random();

    // Each substream closes before the client sends its handshake.
    for _ in 0..3 {
        drop(server.dial_peer(peer_id));
    }

    let mut client = connect(server.dial_peer(peer_id)).await.unwrap();
    client.say_hello(SayHelloRequest::default()).await.unwrap();
}

/// Opens sessions until one is accepted. A slot is released when the server's task for that session
/// finishes, which trails the client closing its end.
async fn wait_for_session(server: &TestRpcServer) -> GreetingClient {
    for _ in 0..50 {
        if let Ok(mut client) = connect(server.dial()).await &&
            client.say_hello(SayHelloRequest::default()).await.is_ok()
        {
            return client;
        }
        time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the server never released the closed session's slot");
}
