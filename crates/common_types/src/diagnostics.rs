//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Types for the validator's local diagnostic event log.
//!
//! The log is local observability only: nothing here participates in consensus, is gossiped to
//! peers, or affects state. `topic` is a free-form dotted string and `fields` a flat string map so
//! that a new event kind never changes the stored schema — rows written by an earlier binary always
//! decode.

use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
    time::{SystemTime, UNIX_EPOCH},
};

use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};

/// Severity of a diagnostic event. Ordered, so a "minimum level" filter is a comparison.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Encode, Decode, CborLen,
)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    #[default]
    #[n(0)]
    Info,
    #[n(1)]
    Warn,
    #[n(2)]
    Error,
}

impl Display for DiagnosticLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagnosticLevel::Info => write!(f, "info"),
            DiagnosticLevel::Warn => write!(f, "warn"),
            DiagnosticLevel::Error => write!(f, "error"),
        }
    }
}

impl std::str::FromStr for DiagnosticLevel {
    type Err = InvalidDiagnosticLevel;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "info" => Ok(DiagnosticLevel::Info),
            "warn" | "warning" => Ok(DiagnosticLevel::Warn),
            "error" => Ok(DiagnosticLevel::Error),
            _ => Err(InvalidDiagnosticLevel(s.to_string())),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid diagnostic level '{0}'. Expected one of: info, warn, error")]
pub struct InvalidDiagnosticLevel(String);

/// A single recorded occurrence of something worth knowing about when diagnosing a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode, CborLen)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiagnosticEvent {
    /// Unix milliseconds at which the event was emitted (not at which it was persisted).
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    #[n(0)]
    pub timestamp: u64,
    #[n(1)]
    pub level: DiagnosticLevel,
    /// Dotted namespace, e.g. `consensus.leader_failure`. Stable across releases so that operator
    /// filters and alerts keep working.
    #[n(2)]
    pub topic: String,
    /// Human-readable one-liner.
    #[n(3)]
    pub message: String,
    /// Structured context (epoch, height, block_id, peer, error, ...) rendered as strings.
    #[n(4)]
    pub fields: BTreeMap<String, String>,
}

impl DiagnosticEvent {
    pub fn new<T: Into<String>, M: Into<String>>(level: DiagnosticLevel, topic: T, message: M) -> Self {
        Self {
            timestamp: unix_millis_now(),
            level,
            topic: topic.into(),
            message: message.into(),
            fields: BTreeMap::new(),
        }
    }

    pub fn info<T: Into<String>, M: Into<String>>(topic: T, message: M) -> Self {
        Self::new(DiagnosticLevel::Info, topic, message)
    }

    pub fn warn<T: Into<String>, M: Into<String>>(topic: T, message: M) -> Self {
        Self::new(DiagnosticLevel::Warn, topic, message)
    }

    pub fn error<T: Into<String>, M: Into<String>>(topic: T, message: M) -> Self {
        Self::new(DiagnosticLevel::Error, topic, message)
    }

    pub fn with_field<K: Into<String>, V: Display>(mut self, key: K, value: V) -> Self {
        self.fields.insert(key.into(), value.to_string());
        self
    }

    /// Adds `key` only if `value` is `Some`, so optional context doesn't need a branch at the call
    /// site.
    pub fn with_optional_field<K: Into<String>, V: Display>(self, key: K, value: Option<V>) -> Self {
        match value {
            Some(value) => self.with_field(key, value),
            None => self,
        }
    }
}

/// A [`DiagnosticEvent`] as stored, carrying the sequence number assigned on insert.
///
/// `id` increases with insertion order, which is what makes it usable as a pagination cursor. It is
/// derived from the highest id held, so emptying the log restarts the sequence at zero; ids identify
/// an event within the log's current contents, not for all time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiagnosticEventRecord {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub id: u64,
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(flatten))]
    pub event: DiagnosticEvent,
}

/// Filter applied to a list or clear request. All set fields must match (logical AND); an
/// all-default query matches every event.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiagnosticEventFilter {
    /// Excludes events below this level.
    #[serde(default)]
    pub min_level: Option<DiagnosticLevel>,
    /// Matches topics starting with this string, e.g. `consensus.` for all consensus events.
    #[serde(default)]
    pub topic_prefix: Option<String>,
    /// Unix milliseconds, inclusive.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    #[serde(default)]
    pub since: Option<u64>,
    /// Unix milliseconds, inclusive.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    #[serde(default)]
    pub until: Option<u64>,
}

impl DiagnosticEventFilter {
    pub fn matches(&self, event: &DiagnosticEvent) -> bool {
        if self.min_level.is_some_and(|min| event.level < min) {
            return false;
        }
        if self
            .topic_prefix
            .as_ref()
            .is_some_and(|prefix| !event.topic.starts_with(prefix.as_str()))
        {
            return false;
        }
        if self.since.is_some_and(|since| event.timestamp < since) {
            return false;
        }
        if self.until.is_some_and(|until| event.timestamp > until) {
            return false;
        }
        true
    }

    pub fn is_match_all(&self) -> bool {
        self.min_level.is_none() && self.topic_prefix.is_none() && self.since.is_none() && self.until.is_none()
    }
}

/// Receives diagnostic events from a subsystem that has no access to the state store.
///
/// Implementations must never block and never fail: an event that cannot be recorded is dropped.
pub trait DiagnosticSink: Send + Sync + 'static {
    fn emit(&self, event: DiagnosticEvent);
}

/// Discards every event. The default for subsystems where diagnostics are not wired up, such as
/// tests and the indexer.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopSink;

impl DiagnosticSink for NoopSink {
    fn emit(&self, _event: DiagnosticEvent) {}
}

impl<T: DiagnosticSink + ?Sized> DiagnosticSink for std::sync::Arc<T> {
    fn emit(&self, event: DiagnosticEvent) {
        (**self).emit(event);
    }
}

pub fn unix_millis_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Builds a [`DiagnosticEvent`] with optional `key => value` context fields.
///
/// ```ignore
/// diag_event!(warn, "consensus.leader_failure", "Leader failed at height {height}", height => height, epoch => epoch);
/// ```
#[macro_export]
macro_rules! diag_event {
    ($level:ident, $topic:expr, $($msg:tt)+) => {
        $crate::diag_event!(@split $level, $topic, [] $($msg)+)
    };

    (@split $level:ident, $topic:expr, [$($fmt:tt)*] $key:ident => $value:expr $(, $rest_key:ident => $rest_value:expr)* $(,)?) => {
        $crate::diagnostics::DiagnosticEvent::$level($topic, format!($($fmt)*))
            .with_field(stringify!($key), $value)
            $(.with_field(stringify!($rest_key), $rest_value))*
    };

    (@split $level:ident, $topic:expr, [$($fmt:tt)*] $next:tt $($rest:tt)*) => {
        $crate::diag_event!(@split $level, $topic, [$($fmt)* $next] $($rest)*)
    };

    (@split $level:ident, $topic:expr, [$($fmt:tt)*]) => {
        $crate::diagnostics::DiagnosticEvent::$level($topic, format!($($fmt)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_ordering_supports_min_level_filtering() {
        assert!(DiagnosticLevel::Info < DiagnosticLevel::Warn);
        assert!(DiagnosticLevel::Warn < DiagnosticLevel::Error);
    }

    #[test]
    fn cbor_round_trip_preserves_unknown_topics_and_empty_fields() {
        let event = DiagnosticEvent::warn("some.future.topic", "hello");
        let bytes = tari_bor::encode(&event).unwrap();
        let decoded: DiagnosticEvent = tari_bor::decode_exact(&bytes).unwrap();
        assert_eq!(decoded, event);
        assert!(decoded.fields.is_empty());
    }

    #[test]
    fn cbor_round_trip_preserves_fields() {
        let event = DiagnosticEvent::error("consensus.error", "boom")
            .with_field("epoch", 12)
            .with_optional_field("height", Some(44))
            .with_optional_field::<_, u64>("block_id", None);
        let bytes = tari_bor::encode(&event).unwrap();
        let decoded: DiagnosticEvent = tari_bor::decode_exact(&bytes).unwrap();
        assert_eq!(decoded, event);
        assert_eq!(decoded.fields.get("epoch").unwrap(), "12");
        assert_eq!(decoded.fields.get("height").unwrap(), "44");
        assert!(!decoded.fields.contains_key("block_id"));
    }

    #[test]
    fn filter_matches_on_every_dimension() {
        let mut event = DiagnosticEvent::warn("consensus.leader_failure", "msg");
        event.timestamp = 1_000;

        assert!(DiagnosticEventFilter::default().matches(&event));
        assert!(
            DiagnosticEventFilter {
                min_level: Some(DiagnosticLevel::Warn),
                topic_prefix: Some("consensus.".to_string()),
                since: Some(1_000),
                until: Some(1_000),
            }
            .matches(&event)
        );
        assert!(
            !DiagnosticEventFilter {
                min_level: Some(DiagnosticLevel::Error),
                ..Default::default()
            }
            .matches(&event)
        );
        assert!(
            !DiagnosticEventFilter {
                topic_prefix: Some("sync.".to_string()),
                ..Default::default()
            }
            .matches(&event)
        );
        assert!(
            !DiagnosticEventFilter {
                since: Some(1_001),
                ..Default::default()
            }
            .matches(&event)
        );
        assert!(
            !DiagnosticEventFilter {
                until: Some(999),
                ..Default::default()
            }
            .matches(&event)
        );
    }

    #[test]
    fn macro_builds_message_and_fields() {
        let height = 7u64;
        let event = crate::diag_event!(warn, "consensus.leader_failure", "Leader failed at {height}", height => height, epoch => 3);
        assert_eq!(event.level, DiagnosticLevel::Warn);
        assert_eq!(event.message, "Leader failed at 7");
        assert_eq!(event.fields.get("height").unwrap(), "7");
        assert_eq!(event.fields.get("epoch").unwrap(), "3");

        let event = crate::diag_event!(info, "node.started", "up");
        assert_eq!(event.message, "up");
        assert!(event.fields.is_empty());
    }
}
