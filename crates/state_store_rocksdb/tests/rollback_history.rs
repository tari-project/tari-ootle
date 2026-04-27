//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for the rollback-history breadcrumb CF.
//!
//! The CF and its `RollbackHistoryEntry` value type are registered in the rocksdb
//! schema (this crate), but reads and writes go through `tari_validator_rollback`'s
//! storage module. We test from here because the helpers and seeded DB live here.

pub mod helpers;

use helpers::create_rocksdb;
use tari_ootle_common_types::{Epoch, NumPreshards, ShardGroup, shard::Shard};
use tari_ootle_storage::{StateStore, consensus_models::RollbackHistoryEntry};
use tari_validator_rollback::storage::{rollback_history_insert, rollback_history_list};

fn sample_entry(target_epoch: u64, applied_at_unix_secs: u64) -> RollbackHistoryEntry {
    RollbackHistoryEntry {
        target_epoch: Epoch(target_epoch),
        shard_group: ShardGroup::all_shards(NumPreshards::P4),
        applied_at_unix_secs,
        tool_version: "0.1.0".to_string(),
        audit_file_basename: format!("rollback-audit-{target_epoch}-{applied_at_unix_secs}.bin"),
    }
}

#[test]
fn insert_and_list_chronological() {
    let (db, _tmp) = create_rocksdb();
    let earlier = sample_entry(5, 1_700_000_000);
    let later = sample_entry(7, 1_700_100_000);

    db.with_write_tx(|tx| rollback_history_insert(tx, &later)).unwrap();
    db.with_write_tx(|tx| rollback_history_insert(tx, &earlier)).unwrap();

    let listed = db.with_read_tx(rollback_history_list).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].applied_at_unix_secs, earlier.applied_at_unix_secs);
    assert_eq!(listed[1].applied_at_unix_secs, later.applied_at_unix_secs);
    assert_eq!(listed[0].target_epoch, Epoch(5));
    assert_eq!(listed[1].target_epoch, Epoch(7));
    assert_eq!(listed[0].audit_file_basename, earlier.audit_file_basename);
}

#[test]
fn list_empty_returns_empty_vec() {
    let (db, _tmp) = create_rocksdb();
    let listed = db.with_read_tx(rollback_history_list).unwrap();
    assert!(listed.is_empty());
}

#[test]
fn two_inserts_same_second_different_epoch_both_survive() {
    // Compound key prevents collisions when two rollbacks land in the same second.
    let (db, _tmp) = create_rocksdb();
    let a = sample_entry(5, 1_700_000_000);
    let b = sample_entry(6, 1_700_000_000);

    db.with_write_tx(|tx| rollback_history_insert(tx, &a)).unwrap();
    db.with_write_tx(|tx| rollback_history_insert(tx, &b)).unwrap();

    let listed = db.with_read_tx(rollback_history_list).unwrap();
    assert_eq!(listed.len(), 2);
    // Epoch is the tiebreaker when unix_secs match; smaller epoch first.
    assert_eq!(listed[0].target_epoch, Epoch(5));
    assert_eq!(listed[1].target_epoch, Epoch(6));
}

#[test]
fn shard_group_and_metadata_round_trip() {
    let (db, _tmp) = create_rocksdb();
    let entry = RollbackHistoryEntry {
        target_epoch: Epoch(42),
        shard_group: ShardGroup::new_checked(Shard::from_u32(4), Shard::from_u32(7)).unwrap(),
        applied_at_unix_secs: 1_800_000_000,
        tool_version: "1.2.3-rc.1".to_string(),
        audit_file_basename: "rollback-audit-42-1800000000.bin".to_string(),
    };
    db.with_write_tx(|tx| rollback_history_insert(tx, &entry)).unwrap();
    let listed = db.with_read_tx(rollback_history_list).unwrap();
    assert_eq!(listed.len(), 1);
    let stored = &listed[0];
    assert_eq!(stored.target_epoch, entry.target_epoch);
    assert_eq!(stored.shard_group.start(), entry.shard_group.start());
    assert_eq!(stored.shard_group.end(), entry.shard_group.end());
    assert_eq!(stored.tool_version, entry.tool_version);
    assert_eq!(stored.audit_file_basename, entry.audit_file_basename);
}
