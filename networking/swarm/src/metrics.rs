//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub use libp2p::metrics::Recorder;

use crate::TariNodeBehaviourEvent;

pub struct Metrics {
    libp2p_metrics: libp2p::metrics::Metrics,
    extended_metrics: ExtendedMetrics,
}

impl Metrics {
    pub fn new(registry: &mut libp2p::metrics::Registry) -> Self {
        let libp2p_metrics = libp2p::metrics::Metrics::new(registry);
        let extended_metrics = ExtendedMetrics::new(registry);
        Self {
            libp2p_metrics,
            extended_metrics,
        }
    }
}

impl<TCodec: libp2p_messaging::Codec + Send + Clone + 'static> Recorder<TariNodeBehaviourEvent<TCodec>> for Metrics {
    fn record(&self, event: &TariNodeBehaviourEvent<TCodec>) {
        match event {
            TariNodeBehaviourEvent::Ping(ping_event) => self.libp2p_metrics.record(ping_event),
            TariNodeBehaviourEvent::Dcutr(dcutr_event) => self.libp2p_metrics.record(dcutr_event),
            TariNodeBehaviourEvent::Identify(identify_event) => self.libp2p_metrics.record(identify_event),
            TariNodeBehaviourEvent::RelayClient(_) => {}, // Metrics not implemented for RelayClient
            TariNodeBehaviourEvent::Relay(relay_event) => self.libp2p_metrics.record(relay_event),
            TariNodeBehaviourEvent::Gossipsub(gossipsub_event) => self.libp2p_metrics.record(gossipsub_event),
            TariNodeBehaviourEvent::Messaging(messaging_event) => self.extended_metrics.record(messaging_event),
            TariNodeBehaviourEvent::Substream(substream_event) => self.extended_metrics.record(substream_event),
            TariNodeBehaviourEvent::ConnectionLimits(_) => {}, // No events for connection limits
            TariNodeBehaviourEvent::Mdns(_) => {},             // No metrics for mDNS events
            TariNodeBehaviourEvent::Autonat(_) => {},          // No metrics for Autonat events
            TariNodeBehaviourEvent::PeerSync(peer_sync_event) => self.extended_metrics.record(peer_sync_event),
        }
    }
}

pub struct ExtendedMetrics {
    substream: libp2p_substream::metrics::Metrics,
    messaging: libp2p_messaging::metrics::Metrics,
    peer_sync: libp2p_peersync::metrics::Metrics,
}

impl ExtendedMetrics {
    pub fn new(registry: &mut libp2p::metrics::Registry) -> Self {
        let registry = registry.sub_registry_with_prefix("libp2p_ext");
        Self {
            substream: libp2p_substream::metrics::Metrics::new(registry),
            messaging: libp2p_messaging::metrics::Metrics::new(registry),
            peer_sync: libp2p_peersync::metrics::Metrics::new(registry),
        }
    }
}

impl Recorder<libp2p_substream::Event> for ExtendedMetrics {
    fn record(&self, event: &libp2p_substream::Event) {
        self.substream.record(event);
    }
}

impl<TMsg> Recorder<libp2p_messaging::Event<TMsg>> for ExtendedMetrics {
    fn record(&self, event: &libp2p_messaging::Event<TMsg>) {
        self.messaging.record(event);
    }
}

impl Recorder<libp2p_peersync::Event> for ExtendedMetrics {
    fn record(&self, event: &libp2p_peersync::Event) {
        self.peer_sync.record(event);
    }
}
