//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use prometheus_client::{
    metrics::{counter::Counter, gauge::Gauge},
    registry::Registry,
};

use crate::{Event, ProtocolEvent};

pub struct Metrics {
    num_open_substreams: Gauge,
    inbound_open_count: Counter,
    inbound_failure_count: Counter,
    outbound_failure_count: Counter,
    error_count: Counter,
}

impl Metrics {
    pub fn new(registry: &mut Registry) -> Self {
        let registry = registry.sub_registry_with_prefix("substream");
        let num_open_substreams = Gauge::default();
        registry.register(
            "num_open_substreams",
            "Number of open substreams",
            num_open_substreams.clone(),
        );

        let inbound_open_count = Counter::default();
        registry.register(
            "inbound_open_count",
            "Number of inbound substreams opened",
            inbound_open_count.clone(),
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

        let error_count = Counter::default();
        registry.register(
            "error_count",
            "Number of errors encountered in substream operations",
            error_count.clone(),
        );

        Self {
            num_open_substreams,
            inbound_open_count,
            inbound_failure_count,
            outbound_failure_count,
            error_count,
        }
    }
}

impl libp2p::metrics::Recorder<Event> for Metrics {
    fn record(&self, event: &Event) {
        match event {
            Event::SubstreamOpen {
                peer_id,
                stream_id,
                protocol,
                ..
            } => {
                self.num_open_substreams.inc();
                tracing::info!(
                    "Substream opened for peer {} on protocol {:?} with stream ID {}",
                    peer_id,
                    protocol,
                    stream_id
                );
            },
            Event::InboundSubstreamOpen { notification } => match notification.event {
                ProtocolEvent::NewInboundSubstream { peer_id, .. } => {
                    self.inbound_open_count.inc();
                    tracing::info!(
                        "New inbound substream opened for peer {} on protocol {}",
                        peer_id,
                        notification.protocol
                    );
                },
            },
            Event::InboundFailure {
                peer_id,
                stream_id,
                error,
            } => {
                self.inbound_failure_count.inc();
                tracing::error!(error = ?error, "Inbound substream failure for peer {peer_id} (ID = {stream_id})");
            },
            Event::OutboundFailure {
                peer_id,
                protocol,
                stream_id,
                error,
            } => {
                self.outbound_failure_count.inc();
                tracing::error!(error = ?error, "Outbound substream failure ({stream_id}) peer {peer_id} on protocol {protocol}");
            },
        }

        self.error_count.inc();
    }
}
