//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use prometheus_client::{metrics::counter::Counter, registry::Registry};

#[derive(Debug, Clone)]
pub struct Metrics {
    cache_hits: Counter,
    cache_misses: Counter,
}

impl Metrics {
    pub fn register(registry: &mut Registry) -> Self {
        let registry = registry.sub_registry_with_prefix("substate_scanner");
        Self {
            cache_hits: {
                let metric = Counter::default();
                registry.register("cache_hits", "Number of cache hits", metric.clone());
                metric
            },
            cache_misses: {
                let metric = Counter::default();
                registry.register("cache_misses", "Number of cache misses", metric.clone());
                metric
            },
        }
    }

    pub fn inc_cache_hits(&self) {
        self.cache_hits.inc();
    }

    pub fn inc_cache_misses(&self) {
        self.cache_misses.inc();
    }
}
