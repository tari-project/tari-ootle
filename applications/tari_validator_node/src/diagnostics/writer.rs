//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use log::*;
use tari_ootle_common_types::diagnostics::DiagnosticEvent;
use tari_ootle_storage::{DiagnosticEventStore, DiagnosticRetention};
use tari_shutdown::ShutdownSignal;
use tokio::{sync::mpsc, task::JoinHandle, time};

use crate::{config::DiagnosticsConfig, diagnostics::handle::DiagnosticsHandle};

const LOG_TARGET: &str = "tari::validator_node::diagnostics";

/// Bounds the backlog of events waiting to be written. Emitters never block on a full channel, so
/// this only decides how large a burst is absorbed before events start being dropped.
const CHANNEL_CAPACITY: usize = 1024;
/// Events are batched into one write transaction up to this many at a time.
const MAX_BATCH: usize = 64;
const FLUSH_INTERVAL: Duration = Duration::from_millis(200);

/// Starts the background writer. Returns a handle for emitters and the task's join handle; when
/// diagnostics are disabled the handle discards everything and no task is spawned.
pub fn spawn<TStore>(
    store: TStore,
    config: &DiagnosticsConfig,
    shutdown: ShutdownSignal,
) -> (DiagnosticsHandle, Option<JoinHandle<Result<(), anyhow::Error>>>)
where
    TStore: DiagnosticEventStore + Send + Sync + 'static,
{
    if !config.enabled {
        info!(target: LOG_TARGET, "Diagnostic event log is disabled");
        return (DiagnosticsHandle::disabled(), None);
    }

    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
    let handle = DiagnosticsHandle::new(tx, config.min_level);
    let writer = Writer {
        store,
        rx,
        handle: handle.clone(),
        retention: config.retention(),
    };

    let join_handle = tokio::spawn(async move {
        writer.run(shutdown).await;
        Ok(())
    });

    (handle, Some(join_handle))
}

struct Writer<TStore> {
    store: TStore,
    rx: mpsc::Receiver<DiagnosticEvent>,
    handle: DiagnosticsHandle,
    retention: DiagnosticRetention,
}

impl<TStore: DiagnosticEventStore> Writer<TStore> {
    async fn run(mut self, mut shutdown: ShutdownSignal) {
        info!(
            target: LOG_TARGET,
            "📓 Diagnostic event log started (keeping up to {} events for up to {}s)",
            self.retention.max_events,
            self.retention.max_age.as_secs()
        );

        let mut interval = time::interval(FLUSH_INTERVAL);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        let mut batch = Vec::with_capacity(MAX_BATCH);

        loop {
            tokio::select! {
                biased;
                _ = shutdown.wait() => {
                    // Drain whatever is already queued: the last events before a shutdown are
                    // usually the interesting ones.
                    while let Ok(event) = self.rx.try_recv() {
                        batch.push(event);
                    }
                    self.flush(&mut batch);
                    break;
                },
                received = self.rx.recv_many(&mut batch, MAX_BATCH) => {
                    if received == 0 {
                        self.flush(&mut batch);
                        break;
                    }
                    if batch.len() >= MAX_BATCH {
                        self.flush(&mut batch);
                    }
                },
                _ = interval.tick() => {
                    self.flush(&mut batch);
                },
            }
        }

        info!(target: LOG_TARGET, "💤 Diagnostic event log stopped");
    }

    fn flush(&self, batch: &mut Vec<DiagnosticEvent>) {
        let dropped = self.handle.take_dropped();
        if dropped > 0 {
            batch.push(
                DiagnosticEvent::warn(
                    "diagnostics.dropped",
                    format!("{dropped} diagnostic event(s) were dropped because the writer fell behind"),
                )
                .with_field("count", dropped),
            );
        }

        if batch.is_empty() {
            return;
        }

        if let Err(err) = self.store.diagnostic_events_append(batch) {
            // Losing diagnostics must never take the node down, and re-emitting the failure as an
            // event would just fail the same way.
            warn!(target: LOG_TARGET, "Failed to write {} diagnostic event(s): {err}", batch.len());
        }
        batch.clear();

        match self.store.diagnostic_events_prune(self.retention) {
            Ok(stats) if stats.total() > 0 => {
                debug!(
                    target: LOG_TARGET,
                    "Pruned {} diagnostic event(s) ({} over the count bound, {} expired)",
                    stats.total(),
                    stats.deleted_by_count,
                    stats.deleted_by_age
                );
            },
            Ok(_) => {},
            Err(err) => warn!(target: LOG_TARGET, "Failed to prune diagnostic events: {err}"),
        }
    }
}
