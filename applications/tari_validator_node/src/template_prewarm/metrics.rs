//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use prometheus_client::{
    metrics::{
        counter::Counter,
        gauge::Gauge,
        histogram::{Histogram, exponential_buckets},
    },
    registry::Registry,
};

use crate::metrics::CollectorRegister;

/// Counters are shared by every clone, so a worker and the handle that queued its work report to the
/// same series.
#[derive(Debug, Clone)]
pub struct PrometheusPrewarmMetrics {
    enqueued: Counter,
    dropped: Counter,
    loaded: Counter,
    already_resident: Counter,
    not_found: Counter,
    failed: Counter,
    queue_depth: Gauge,
    load_seconds: Histogram,
}

impl PrometheusPrewarmMetrics {
    pub fn new(registry: &mut Registry) -> Self {
        let registry = registry.sub_registry_with_prefix("template_prewarm");
        Self {
            enqueued: Counter::default().register_at(
                "enqueued",
                "Number of templates queued for background compilation",
                registry,
            ),
            dropped: Counter::default().register_at(
                "dropped",
                "Number of prewarm requests dropped because the queue was full",
                registry,
            ),
            loaded: Counter::default().register_at(
                "loaded",
                "Number of templates the prewarm pool made resident, whether it compiled them or read an artifact the \
                 disk cache already held",
                registry,
            ),
            already_resident: Counter::default().register_at(
                "already_resident",
                "Number of prewarm requests whose template was resident by the time a worker reached it, most often \
                 because execution compiled it first",
                registry,
            ),
            not_found: Counter::default().register_at(
                "not_found",
                "Number of prewarm requests for a template this node does not hold, which is what a node still \
                 catching up is expected to see",
                registry,
            ),
            failed: Counter::default().register_at(
                "failed",
                "Number of prewarm requests whose template this node holds but could not load",
                registry,
            ),
            queue_depth: Gauge::default().register_at(
                "queue_depth",
                "Targets queued for background compilation, including the one each worker is compiling",
                registry,
            ),
            // A worker asks the provider chain for a template and is not told which tier answered,
            // so the two kinds of work this pool does are told apart by how long they take: reading
            // an artifact off disk is around a millisecond, a Cranelift compile is tens to hundreds.
            // Counting them together would hide the only number the pool exists to move. Buckets run
            // from 1 ms so the fast mode is resolved rather than piled into the first one.
            load_seconds: Histogram::new(exponential_buckets(0.001, 2.0, 12)).register_at(
                "load_seconds",
                "Time a prewarm worker spent making one template resident, in seconds",
                registry,
            ),
        }
    }

    pub fn on_enqueued(&self, queue_depth: usize) {
        self.enqueued.inc();
        self.set_queue_depth(queue_depth);
    }

    pub fn on_finished(&self, queue_depth: usize) {
        self.set_queue_depth(queue_depth);
    }

    pub fn on_dropped(&self) {
        self.dropped.inc();
    }

    pub fn on_loaded(&self, elapsed: Duration) {
        self.loaded.inc();
        self.load_seconds.observe(elapsed.as_secs_f64());
    }

    pub fn on_already_resident(&self) {
        self.already_resident.inc();
    }

    pub fn on_not_found(&self) {
        self.not_found.inc();
    }

    pub fn on_failed(&self) {
        self.failed.inc();
    }

    fn set_queue_depth(&self, depth: usize) {
        self.queue_depth.set(i64::try_from(depth).unwrap_or(i64::MAX));
    }
}
