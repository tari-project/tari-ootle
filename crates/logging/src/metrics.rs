//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use prometheus_client::{
    collector::Collector,
    encoding::{DescriptorEncoder, EncodeMetric},
    metrics::counter::ConstCounter,
    registry::Registry,
};

use crate::dropped_lines;

/// Reports the lines each log sink dropped because its writer fell behind, labelled by `sink`, read from the sinks
/// on each scrape.
#[derive(Debug)]
pub struct LogSinkCollector;

impl LogSinkCollector {
    /// Registers under the `log` sub-registry, so the metric on the wire is `log_dropped_lines_total`.
    pub fn register(self, registry: &mut Registry) {
        let registry = registry.sub_registry_with_prefix("log");
        registry.register_collector(Box::new(self));
    }
}

impl Collector for LogSinkCollector {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), fmt::Error> {
        let mut family = encoder.encode_descriptor(
            "dropped_lines",
            "Log lines discarded because the sink's writer fell behind",
            None,
            ConstCounter::<u64>::new(0).metric_type(),
        )?;
        for (sink, dropped) in dropped_lines() {
            ConstCounter::<u64>::new(dropped as u64).encode(family.encode_family(&[("sink", sink)])?)?;
        }
        Ok(())
    }
}
