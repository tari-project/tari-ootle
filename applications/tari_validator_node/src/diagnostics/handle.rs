//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use tari_ootle_common_types::diagnostics::{DiagnosticEvent, DiagnosticLevel, DiagnosticSink};
use tokio::sync::mpsc;

/// The emit side of the diagnostic event log. Cheap to clone and safe to call from any thread:
/// emitting never blocks, never fails, and never touches the database.
#[derive(Debug, Clone)]
pub struct DiagnosticsHandle {
    inner: Option<Arc<Inner>>,
}

#[derive(Debug)]
struct Inner {
    sender: mpsc::Sender<DiagnosticEvent>,
    min_level: DiagnosticLevel,
    dropped: AtomicU64,
}

impl DiagnosticsHandle {
    pub(super) fn new(sender: mpsc::Sender<DiagnosticEvent>, min_level: DiagnosticLevel) -> Self {
        Self {
            inner: Some(Arc::new(Inner {
                sender,
                min_level,
                dropped: AtomicU64::new(0),
            })),
        }
    }

    /// A handle that discards everything, for when diagnostics are switched off.
    pub fn disabled() -> Self {
        Self { inner: None }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    pub fn emit(&self, event: DiagnosticEvent) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        if event.level < inner.min_level {
            return;
        }
        if inner.sender.try_send(event).is_err() {
            inner.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Takes the number of events dropped since this was last called. The writer reports this as an
    /// event of its own so that silent loss is itself visible in the log.
    pub(super) fn take_dropped(&self) -> u64 {
        self.inner
            .as_ref()
            .map_or(0, |inner| inner.dropped.swap(0, Ordering::Relaxed))
    }
}

impl DiagnosticSink for DiagnosticsHandle {
    fn emit(&self, event: DiagnosticEvent) {
        DiagnosticsHandle::emit(self, event);
    }
}
