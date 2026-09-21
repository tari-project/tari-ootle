//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for the diagnostic event log. The log shares the `diagnostics` column family with the
//! no-vote breadcrumbs, so the prefix isolation is tested here too.

pub mod helpers;

use std::time::Duration;

use helpers::{create_rocksdb, create_rocksdb_with_opts};
use tari_ootle_common_types::diagnostics::{DiagnosticEvent, DiagnosticEventFilter, DiagnosticLevel, unix_millis_now};
use tari_ootle_storage::{
    DiagnosticEventPage,
    DiagnosticEventStore,
    DiagnosticRetention,
    Ordering,
    StateStore,
    StateStoreWriteTransaction,
    consensus_models::NoVoteReason,
};
use tari_state_store_rocksdb::{DatabaseOptions, column_families::diagnostic_no_vote::DiagnosticsNoVoteCf};

fn event_at(level: DiagnosticLevel, topic: &str, timestamp: u64) -> DiagnosticEvent {
    let mut event = DiagnosticEvent::new(level, topic, format!("{topic} at {timestamp}"));
    event.timestamp = timestamp;
    event
}

fn page(limit: usize) -> DiagnosticEventPage {
    DiagnosticEventPage {
        limit,
        ..Default::default()
    }
}

fn retention(max_events: usize, max_age: Duration) -> DiagnosticRetention {
    DiagnosticRetention { max_events, max_age }
}

#[test]
fn append_assigns_increasing_ids_and_queries_newest_first() {
    let (db, _tmp) = create_rocksdb();

    let last = db
        .diagnostic_events_append(&[
            event_at(DiagnosticLevel::Info, "node.started", 1_000),
            event_at(DiagnosticLevel::Warn, "consensus.leader_failure", 2_000),
        ])
        .unwrap();
    assert_eq!(last, Some(1));

    let last = db
        .diagnostic_events_append(&[event_at(DiagnosticLevel::Error, "consensus.error", 3_000)])
        .unwrap();
    assert_eq!(last, Some(2));

    let events = db.diagnostic_events_query(&page(10)).unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].id, 2);
    assert_eq!(events[0].event.topic, "consensus.error");
    assert_eq!(events[1].id, 1);
    assert_eq!(events[2].id, 0);
    assert_eq!(db.diagnostic_events_bounds().unwrap(), Some((0, 2)));
}

#[test]
fn append_of_nothing_is_a_no_op() {
    let (db, _tmp) = create_rocksdb();
    assert_eq!(db.diagnostic_events_append(&[]).unwrap(), None);
    assert_eq!(db.diagnostic_events_bounds().unwrap(), None);
    assert!(db.diagnostic_events_query(&page(10)).unwrap().is_empty());
}

#[test]
fn query_filters_on_level_topic_and_time() {
    let (db, _tmp) = create_rocksdb();
    db.diagnostic_events_append(&[
        event_at(DiagnosticLevel::Info, "consensus.state_transition", 1_000),
        event_at(DiagnosticLevel::Warn, "consensus.leader_failure", 2_000),
        event_at(DiagnosticLevel::Error, "sync.failed", 3_000),
    ])
    .unwrap();

    let warn_and_above = db
        .diagnostic_events_query(&DiagnosticEventPage {
            filter: DiagnosticEventFilter {
                min_level: Some(DiagnosticLevel::Warn),
                ..Default::default()
            },
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(warn_and_above.len(), 2);
    assert!(warn_and_above.iter().all(|e| e.event.level >= DiagnosticLevel::Warn));

    let consensus_only = db
        .diagnostic_events_query(&DiagnosticEventPage {
            filter: DiagnosticEventFilter {
                topic_prefix: Some("consensus.".to_string()),
                ..Default::default()
            },
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(consensus_only.len(), 2);
    assert!(consensus_only.iter().all(|e| e.event.topic.starts_with("consensus.")));

    let windowed = db
        .diagnostic_events_query(&DiagnosticEventPage {
            filter: DiagnosticEventFilter {
                since: Some(2_000),
                until: Some(2_000),
                ..Default::default()
            },
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(windowed.len(), 1);
    assert_eq!(windowed[0].event.topic, "consensus.leader_failure");
}

#[test]
fn query_pages_with_the_cursor() {
    let (db, _tmp) = create_rocksdb();
    let events = (0..5)
        .map(|i| event_at(DiagnosticLevel::Info, "node.started", 1_000 + i))
        .collect::<Vec<_>>();
    db.diagnostic_events_append(&events).unwrap();

    let first = db.diagnostic_events_query(&page(2)).unwrap();
    assert_eq!(first.iter().map(|e| e.id).collect::<Vec<_>>(), vec![4, 3]);

    let second = db
        .diagnostic_events_query(&DiagnosticEventPage {
            before_id: Some(first.last().unwrap().id),
            limit: 2,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(second.iter().map(|e| e.id).collect::<Vec<_>>(), vec![2, 1]);

    let third = db
        .diagnostic_events_query(&DiagnosticEventPage {
            before_id: Some(second.last().unwrap().id),
            limit: 2,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(third.iter().map(|e| e.id).collect::<Vec<_>>(), vec![0]);
}

#[test]
fn query_before_the_first_id_returns_nothing() {
    // The cursor bounds the iterator range, so a cursor at the very start is an empty range rather
    // than a scan that finds nothing.
    let (db, _tmp) = create_rocksdb();
    db.diagnostic_events_append(&[event_at(DiagnosticLevel::Info, "node.started", 1_000)])
        .unwrap();

    let events = db
        .diagnostic_events_query(&DiagnosticEventPage {
            before_id: Some(0),
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert!(events.is_empty());
}

#[test]
fn query_with_a_zero_limit_returns_nothing() {
    let (db, _tmp) = create_rocksdb();
    db.diagnostic_events_append(&[event_at(DiagnosticLevel::Info, "node.started", 1_000)])
        .unwrap();
    assert!(db.diagnostic_events_query(&page(0)).unwrap().is_empty());
}

#[test]
fn clear_deletes_only_matching_events() {
    let (db, _tmp) = create_rocksdb();
    db.diagnostic_events_append(&[
        event_at(DiagnosticLevel::Info, "consensus.state_transition", 1_000),
        event_at(DiagnosticLevel::Warn, "consensus.leader_failure", 2_000),
        event_at(DiagnosticLevel::Error, "sync.failed", 3_000),
    ])
    .unwrap();

    let deleted = db
        .diagnostic_events_clear(&DiagnosticEventFilter {
            topic_prefix: Some("consensus.".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(deleted, 2);

    let remaining = db.diagnostic_events_query(&page(10)).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].event.topic, "sync.failed");

    // Ids continue from where they left off so a cursor held by a client never rewinds.
    let last = db
        .diagnostic_events_append(&[event_at(DiagnosticLevel::Info, "node.started", 4_000)])
        .unwrap();
    assert_eq!(last, Some(3));
}

#[test]
fn clear_with_an_empty_filter_deletes_everything() {
    let (db, _tmp) = create_rocksdb();
    db.diagnostic_events_append(&[
        event_at(DiagnosticLevel::Info, "a", 1_000),
        event_at(DiagnosticLevel::Warn, "b", 2_000),
    ])
    .unwrap();

    assert_eq!(
        db.diagnostic_events_clear(&DiagnosticEventFilter::default()).unwrap(),
        2
    );
    assert_eq!(db.diagnostic_events_bounds().unwrap(), None);
}

#[test]
fn emptying_the_log_restarts_the_id_sequence() {
    let (db, _tmp) = create_rocksdb();
    db.diagnostic_events_append(&[
        event_at(DiagnosticLevel::Info, "a", 1_000),
        event_at(DiagnosticLevel::Info, "b", 2_000),
    ])
    .unwrap();
    db.diagnostic_events_clear(&DiagnosticEventFilter::default()).unwrap();

    let last = db
        .diagnostic_events_append(&[event_at(DiagnosticLevel::Info, "c", 3_000)])
        .unwrap();
    assert_eq!(last, Some(0));
}

#[test]
fn prune_enforces_the_count_bound() {
    let (db, _tmp) = create_rocksdb();
    let now = unix_millis_now();
    let events = (0..1_200u64)
        .map(|i| event_at(DiagnosticLevel::Info, "node.started", now + i))
        .collect::<Vec<_>>();
    db.diagnostic_events_append(&events).unwrap();

    let stats = db
        .diagnostic_events_prune(retention(1_000, Duration::from_secs(7 * 24 * 60 * 60)))
        .unwrap();
    assert_eq!(stats.deleted_by_count, 200);
    assert_eq!(stats.deleted_by_age, 0);

    let (oldest, newest) = db.diagnostic_events_bounds().unwrap().unwrap();
    assert_eq!(newest, 1_199);
    assert_eq!(oldest, 200);
    assert_eq!(db.diagnostic_events_query(&page(2_000)).unwrap().len(), 1_000);
}

#[test]
fn prune_enforces_the_age_bound() {
    let (db, _tmp) = create_rocksdb();
    let now = unix_millis_now();
    let hour = 60 * 60 * 1_000;
    db.diagnostic_events_append(&[
        event_at(DiagnosticLevel::Info, "old", now - 48 * hour),
        event_at(DiagnosticLevel::Info, "also_old", now - 25 * hour),
        event_at(DiagnosticLevel::Info, "recent", now - hour),
    ])
    .unwrap();

    let stats = db
        .diagnostic_events_prune(retention(1_000, Duration::from_secs(24 * 60 * 60)))
        .unwrap();
    assert_eq!(stats.deleted_by_age, 2);
    assert_eq!(stats.deleted_by_count, 0);

    let remaining = db.diagnostic_events_query(&page(10)).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].event.topic, "recent");
}

#[test]
fn prune_of_an_empty_log_is_a_no_op() {
    let (db, _tmp) = create_rocksdb();
    let stats = db.diagnostic_events_prune(DiagnosticRetention::default()).unwrap();
    assert_eq!(stats.total(), 0);
}

#[test]
fn prune_with_a_zero_count_keeps_nothing() {
    let (db, _tmp) = create_rocksdb();
    db.diagnostic_events_append(&[
        event_at(DiagnosticLevel::Info, "a", unix_millis_now()),
        event_at(DiagnosticLevel::Info, "b", unix_millis_now()),
    ])
    .unwrap();

    let stats = db
        .diagnostic_events_prune(retention(0, Duration::from_secs(1)))
        .unwrap();
    assert_eq!(stats.total(), 2);
    assert_eq!(db.diagnostic_events_bounds().unwrap(), None);
}

#[test]
fn no_vote_breadcrumbs_in_the_same_cf_are_untouched() {
    let (db, _tmp) = create_rocksdb_with_opts(DatabaseOptions::default().with_debugging_data(true));
    let block_id = tari_consensus_types::BlockId::zero();
    db.with_write_tx(|tx| tx.diagnostics_add_no_vote(block_id, NoVoteReason::AlreadyVotedAtHeight))
        .unwrap();

    db.diagnostic_events_append(&[event_at(DiagnosticLevel::Info, "node.started", unix_millis_now())])
        .unwrap();
    db.diagnostic_events_clear(&DiagnosticEventFilter::default()).unwrap();
    db.diagnostic_events_prune(retention(0, Duration::from_secs(0)))
        .unwrap();

    // The no-vote row shares the column family; reaching it would mean the prefix bound leaked.
    assert!(db.diagnostic_events_bounds().unwrap().is_none());
    let tx = db.create_read_tx().unwrap();
    let no_votes = tx
        .db()
        .cf(DiagnosticsNoVoteCf)
        .unwrap()
        .iterator(Ordering::Ascending, "test")
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(no_votes.len(), 1);
}
