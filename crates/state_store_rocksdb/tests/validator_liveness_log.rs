//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for the validator liveness log: the history that lets leader selection read a validator's
//! state as of a past committed height rather than only the latest one.

pub mod helpers;

use helpers::create_rocksdb;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    consensus_models::{LivenessThresholds, ValidatorConsensusStats, ValidatorStatsUpdate},
};
use tari_state_store_rocksdb::RocksDbStateStore;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

const EPOCH: Epoch = Epoch(7);
const THRESHOLDS: LivenessThresholds = LivenessThresholds {
    suspend_after_missed: 5,
    probation_base_votes: 5,
    probation_base_blocks: 100,
    probation_max_backoff_exp: 6,
};

fn validator(byte: u8) -> RistrettoPublicKeyBytes {
    RistrettoPublicKeyBytes::from_bytes(&[byte; 32]).unwrap()
}

fn miss_proposal(db: &RocksDbStateStore<String>, public_key: &RistrettoPublicKeyBytes, height: u64) {
    db.with_write_tx(|tx| {
        tx.validator_epoch_stats_updates(EPOCH, NodeHeight(height), [ValidatorStatsUpdate::new(
            public_key, THRESHOLDS,
        )
        .add_missed_proposal()])
    })
    .unwrap();
}

fn vote(db: &RocksDbStateStore<String>, public_key: &RistrettoPublicKeyBytes, height: u64) {
    db.with_write_tx(|tx| {
        tx.validator_epoch_stats_updates(EPOCH, NodeHeight(height), [ValidatorStatsUpdate::new(
            public_key, THRESHOLDS,
        )
        .record_vote()])
    })
    .unwrap();
}

fn missed_as_of(db: &RocksDbStateStore<String>, public_key: &RistrettoPublicKeyBytes, height: u64) -> u64 {
    let tx = db.create_read_tx().unwrap();
    ValidatorConsensusStats::liveness_counters_as_of(&tx, EPOCH, public_key, NodeHeight(height))
        .unwrap()
        .missed_proposals
}

#[test]
fn a_validator_with_no_history_reads_as_unchanged() {
    let (db, _tmp) = create_rocksdb();
    let tx = db.create_read_tx().unwrap();
    assert!(
        tx.validator_liveness_counters_as_of(EPOCH, &validator(1), NodeHeight(100))
            .unwrap()
            .is_none()
    );
}

#[test]
fn the_state_as_of_a_height_is_the_last_entry_at_or_below_it() {
    let (db, _tmp) = create_rocksdb();
    let validator = validator(1);
    miss_proposal(&db, &validator, 10);
    miss_proposal(&db, &validator, 20);
    miss_proposal(&db, &validator, 30);

    assert_eq!(missed_as_of(&db, &validator, 9), 0);
    assert_eq!(missed_as_of(&db, &validator, 10), 1);
    assert_eq!(missed_as_of(&db, &validator, 19), 1);
    assert_eq!(missed_as_of(&db, &validator, 20), 2);
    assert_eq!(missed_as_of(&db, &validator, 30), 3);
    // A query above every entry reads the latest one: a node further ahead answers a past query the
    // same way a node at that height does.
    assert_eq!(missed_as_of(&db, &validator, 1_000), 3);
}

#[test]
fn one_validators_history_is_not_read_for_another() {
    let (db, _tmp) = create_rocksdb();
    let (one, two) = (validator(1), validator(2));
    miss_proposal(&db, &one, 10);
    miss_proposal(&db, &one, 11);
    miss_proposal(&db, &two, 12);

    assert_eq!(missed_as_of(&db, &one, 12), 2);
    assert_eq!(missed_as_of(&db, &two, 12), 1);
}

#[test]
fn epochs_do_not_share_a_history() {
    let (db, _tmp) = create_rocksdb();
    let validator = validator(1);
    miss_proposal(&db, &validator, 10);

    let tx = db.create_read_tx().unwrap();
    assert!(
        tx.validator_liveness_counters_as_of(EPOCH + Epoch(1), &validator, NodeHeight(10))
            .unwrap()
            .is_none()
    );
}

/// Participation shares move on every commit for every signer. Logging them would make the log as
/// large as the chain, and they decide nothing about leader selection.
#[test]
fn a_vote_that_moves_no_liveness_counter_writes_no_entry() {
    let (db, _tmp) = create_rocksdb();
    let validator = validator(1);
    vote(&db, &validator, 10);

    let tx = db.create_read_tx().unwrap();
    assert!(
        tx.validator_liveness_counters_as_of(EPOCH, &validator, NodeHeight(10))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        tx.validator_epoch_stats_get(EPOCH, &validator)
            .unwrap()
            .participation_shares,
        1
    );
}

/// Once suspended, votes count towards the probation slot, so they do move a logged counter.
#[test]
fn votes_while_suspended_are_logged() {
    let (db, _tmp) = create_rocksdb();
    let validator = validator(1);
    for height in 1..=5 {
        miss_proposal(&db, &validator, height);
    }
    vote(&db, &validator, 6);

    let tx = db.create_read_tx().unwrap();
    let counters = tx
        .validator_liveness_counters_as_of(EPOCH, &validator, NodeHeight(6))
        .unwrap()
        .unwrap();
    assert_eq!(counters.missed_proposals, 5);
    assert_eq!(counters.votes_since_suspended, 1);
}
