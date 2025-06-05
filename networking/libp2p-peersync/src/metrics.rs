//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use prometheus_client::{
    metrics::{counter::Counter, gauge::Gauge},
    registry::Registry,
};

use crate::Event;

pub struct Metrics {
    inbound_failure_count: Counter,
    outbound_failure_count: Counter,
    num_peers_received: Counter,
    num_inbound_interrupted: Counter,
    num_outbound_interrupted: Counter,
    num_open_substreams: Gauge,
    error_count: Counter,
}

impl Metrics {
    pub fn new(registry: &mut Registry) -> Self {
        let registry = registry.sub_registry_with_prefix("peersync");

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
        let num_peers_received = Counter::default();
        registry.register(
            "num_peers_received",
            "Number of peers received from a peer batch",
            num_peers_received.clone(),
        );
        let num_inbound_interrupted = Counter::default();
        registry.register(
            "num_inbound_interrupted",
            "Number of inbound substreams interrupted",
            num_inbound_interrupted.clone(),
        );
        let num_outbound_interrupted = Counter::default();
        registry.register(
            "num_outbound_interrupted",
            "Number of outbound substreams interrupted",
            num_outbound_interrupted.clone(),
        );
        let num_open_substreams = Gauge::default();
        registry.register(
            "num_open_substreams",
            "Number of open substreams",
            num_open_substreams.clone(),
        );
        let error_count = Counter::default();
        registry.register(
            "error_count",
            "Number of errors encountered in peersync operations",
            error_count.clone(),
        );

        Self {
            inbound_failure_count,
            outbound_failure_count,
            num_peers_received,
            num_inbound_interrupted,
            num_outbound_interrupted,
            num_open_substreams,
            error_count,
        }
    }
}

impl libp2p::metrics::Recorder<Event> for Metrics {
    fn record(&self, event: &Event) {
        use Event::*;
        match event {
            InboundFailure { peer_id, error } => {
                self.inbound_failure_count.inc();
                tracing::error!("Inbound failure from peer {}: {}", peer_id, error);
            },
            OutboundFailure { peer_id, error } => {
                self.outbound_failure_count.inc();
                tracing::error!("Outbound failure to peer {}: {}", peer_id, error);
            },
            PeerBatchReceived { from_peer, new_peers } => {
                self.num_peers_received.inc_by(*new_peers as u64);
                tracing::info!("Received peer batch from {} with {} new peers", from_peer, new_peers);
            },
            InboundStreamInterrupted { peer_id } => {
                self.num_inbound_interrupted.inc();
                tracing::warn!("Inbound stream interrupted for peer {}", peer_id);
            },
            OutboundStreamInterrupted { peer_id } => {
                self.num_outbound_interrupted.inc();
                tracing::warn!("Outbound stream interrupted for peer {}", peer_id);
            },
            InboundRequestCompleted {
                peer_id,
                peers_sent,
                requested,
            } => {
                self.num_open_substreams.dec();
                tracing::info!(
                    "Inbound request completed for peer {}: sent {} peers, requested {}",
                    peer_id,
                    peers_sent,
                    requested
                );
            },
            LocalPeerRecordUpdated => {},
            Error(error) => {
                self.error_count.inc();
                tracing::error!("Error in peersync operation: {}", error);
            },
        }
    }
}
