//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use prometheus_client::{
    metrics::{counter::Counter, gauge::Gauge},
    registry::Registry,
};

use crate::Event;

pub struct Metrics {
    received_message_count: Counter,
    num_messages_sent: Counter,
    inbound_failure_count: Counter,
    outbound_failure_count: Counter,
    inbound_stream_opened_count: Counter,
    outbound_stream_opened_count: Counter,
    num_open_substreams: Gauge,
    error_count: Counter,
}

impl Metrics {
    pub fn new(registry: &mut Registry) -> Self {
        let registry = registry.sub_registry_with_prefix("messaging");

        let received_message_count = Counter::default();
        registry.register(
            "received_message_count",
            "Number of messages received",
            received_message_count.clone(),
        );
        let num_messages_sent = Counter::default();
        registry.register(
            "num_messages_sent",
            "Number of messages sent",
            num_messages_sent.clone(),
        );
        let inbound_failure_count = Counter::default();
        registry.register(
            "inbound_failure_count",
            "Number of inbound substream failures",
            inbound_failure_count.clone(),
        );
        let outbound_failure_count = Counter::default();
        registry.register(
            "outbound_failure_count",
            "Number of outbound substream failures",
            outbound_failure_count.clone(),
        );
        let inbound_stream_opened_count = Counter::default();
        registry.register(
            "inbound_stream_opened_count",
            "Number of inbound streams opened",
            inbound_stream_opened_count.clone(),
        );
        let outbound_stream_opened_count = Counter::default();
        registry.register(
            "outbound_stream_opened_count",
            "Number of outbound streams opened",
            outbound_stream_opened_count.clone(),
        );
        let num_open_substreams = Gauge::default();
        registry.register(
            "num_open_substreams",
            "Number of currently open substreams",
            num_open_substreams.clone(),
        );
        let error_count = Counter::default();
        registry.register(
            "error_count",
            "Number of errors encountered in substream operations",
            error_count.clone(),
        );

        Self {
            received_message_count,
            num_messages_sent,
            inbound_failure_count,
            outbound_failure_count,
            inbound_stream_opened_count,
            outbound_stream_opened_count,
            num_open_substreams,
            error_count,
        }
    }
}

impl<TMsg> libp2p::metrics::Recorder<Event<TMsg>> for Metrics {
    fn record(&self, event: &Event<TMsg>) {
        use Event::*;
        match event {
            ReceivedMessage { peer_id, length, .. } => {
                tracing::debug!("Received message from {}: {} bytes", peer_id, length);
                self.received_message_count.inc();
            },
            MessageSent { message_id, stream_id } => {
                tracing::debug!("Message sent with ID {} on stream {}", message_id, stream_id);
                self.num_messages_sent.inc();
            },
            InboundFailure {
                peer_id,
                stream_id,
                error,
            } => {
                tracing::error!(
                    "Inbound failure for peer {} on stream {}: {}",
                    peer_id,
                    stream_id,
                    error
                );
                self.inbound_failure_count.inc();
            },
            OutboundFailure {
                peer_id,
                stream_id,
                error,
            } => {
                tracing::error!(
                    "Outbound failure for peer {} on stream {}: {}",
                    peer_id,
                    stream_id,
                    error
                );
                self.outbound_failure_count.inc();
            },
            OutboundStreamOpened { peer_id, stream_id } => {
                tracing::debug!(
                    "Outbound stream opened for peer {} with stream ID {}",
                    peer_id,
                    stream_id
                );
                self.outbound_stream_opened_count.inc();
                self.num_open_substreams.inc();
            },
            InboundStreamOpened { peer_id } => {
                tracing::debug!("Inbound stream opened for peer {}", peer_id);
                self.inbound_stream_opened_count.inc();
                self.num_open_substreams.inc();
            },
            InboundStreamClosed { peer_id } => {
                tracing::debug!("Inbound stream closed for peer {}", peer_id);
                self.num_open_substreams.dec();
            },
            StreamClosed { peer_id, stream_id } => {
                tracing::debug!("Stream closed for peer {} with stream ID {}", peer_id, stream_id);
                self.num_open_substreams.dec();
            },
            Error(error) => {
                tracing::error!("Error in substream event: {}", error);
                self.error_count.inc();
            },
        }
    }
}
