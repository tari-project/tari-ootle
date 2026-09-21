//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Access to the validator's local diagnostic event log.
//!
//! This is deliberately separate from [`StateStore`](crate::StateStore): diagnostics are written
//! out-of-band, in their own transaction, so that a consensus write transaction that rolls back
//! still leaves behind the record of the failure that caused the rollback.

use std::time::Duration;

use tari_ootle_common_types::diagnostics::{DiagnosticEvent, DiagnosticEventFilter, DiagnosticEventRecord};

use crate::StorageError;

/// How much history the log keeps. Both bounds are applied on every prune; an event is removed once
/// it falls outside either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticRetention {
    /// Maximum number of events retained. The oldest are dropped first.
    pub max_events: usize,
    /// Maximum age of a retained event.
    pub max_age: Duration,
}

impl Default for DiagnosticRetention {
    fn default() -> Self {
        Self {
            max_events: 10_000,
            max_age: Duration::from_secs(7 * 24 * 60 * 60),
        }
    }
}

/// A page request against the log. Results are always returned newest first.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticEventPage {
    pub filter: DiagnosticEventFilter,
    /// Return only events with an id strictly below this one. Pass the `next_cursor` of the
    /// previous page.
    pub before_id: Option<u64>,
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiagnosticPruneStats {
    pub deleted_by_count: usize,
    pub deleted_by_age: usize,
}

impl DiagnosticPruneStats {
    pub fn total(&self) -> usize {
        self.deleted_by_count + self.deleted_by_age
    }
}

pub trait DiagnosticEventStore {
    /// Appends events in order, assigning each the next sequence number. Returns the id assigned to
    /// the last event, or `None` if `events` was empty.
    fn diagnostic_events_append(&self, events: &[DiagnosticEvent]) -> Result<Option<u64>, StorageError>;

    /// Returns a page of matching events, newest first.
    fn diagnostic_events_query(&self, page: &DiagnosticEventPage) -> Result<Vec<DiagnosticEventRecord>, StorageError>;

    /// Deletes every event matching `filter` and returns how many were deleted.
    fn diagnostic_events_clear(&self, filter: &DiagnosticEventFilter) -> Result<usize, StorageError>;

    /// Enforces `retention`, deleting the events that fall outside it.
    fn diagnostic_events_prune(&self, retention: DiagnosticRetention) -> Result<DiagnosticPruneStats, StorageError>;

    /// Returns the oldest and newest ids held, or `None` if the log is empty.
    fn diagnostic_events_bounds(&self) -> Result<Option<(u64, u64)>, StorageError>;
}
