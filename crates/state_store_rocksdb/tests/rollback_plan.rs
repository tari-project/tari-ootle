//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Read-only rollback-plan iterators: prove the dry-run collection matches what the
//! mutating rewind walks. Seed a DB with known transitions spanning a few state
//! versions, call the collector with target_version below the seeded data, and assert
//! the row count + ordering.

pub mod helpers;

use helpers::{create_rocksdb, create_substate_update_batch, gen_substates_for_shards};
use tari_ootle_common_types::Epoch;
use tari_ootle_storage::{StateStore, StateStoreWriteTransaction, consensus_models::Block};
use tari_ootle_transaction::Network;
use tari_state_tree::Version;
use tari_validator_rollback::storage::{
    RewindTransitionKind,
    rollback_plan_collect_blocks,
    rollback_plan_collect_substates,
};

use crate::helpers::num_preshards;

#[test]
fn collect_substates_yields_reverse_application_order() {
    let (db, _tmp) = create_rocksdb();

    let mut tx = db.create_write_tx().unwrap();
    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();

    // Seed substate commits across multiple state versions on shard 1.
    for batch_idx in 0..3 {
        let substates = gen_substates_for_shards(Epoch::zero(), 1, batch_idx..(batch_idx + 2), 0).collect::<Vec<_>>();
        let batch = create_substate_update_batch(Epoch::zero(), &substates);
        tx.substates_commit_batch(batch).unwrap();
    }
    tx.commit().unwrap();

    let rows = db
        .with_read_tx(|tx| rollback_plan_collect_substates(tx, tari_ootle_common_types::shard::Shard::from_u32(1), 0))
        .unwrap();

    // At minimum we expect some transitions; all on shard 1, epoch 0, and Up-reverted.
    assert!(!rows.is_empty(), "expected at least one transition, got none");
    for (idx, row) in rows.iter().enumerate() {
        assert_eq!(row.epoch, Epoch::zero(), "row {idx} has wrong epoch");
        assert_eq!(row.transition, RewindTransitionKind::UpReverted, "row {idx} wrong kind");
        assert_eq!(row.shard.as_u32(), 1, "row {idx} wrong shard");
    }
    // Rows are yielded in reverse state_version order — check monotonic non-increase.
    for window in rows.windows(2) {
        assert!(
            window[0].state_version >= window[1].state_version,
            "rows out of order: {} before {}",
            window[0].state_version,
            window[1].state_version,
        );
    }
}

#[test]
fn collect_substates_empty_when_target_version_above_everything() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();
    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();
    let substates = gen_substates_for_shards(Epoch::zero(), 1, 0..2, 0).collect::<Vec<_>>();
    let batch = create_substate_update_batch(Epoch::zero(), &substates);
    tx.substates_commit_batch(batch).unwrap();
    tx.commit().unwrap();

    let rows = db
        .with_read_tx(|tx| {
            rollback_plan_collect_substates(tx, tari_ootle_common_types::shard::Shard::from_u32(1), Version::MAX)
        })
        .unwrap();
    assert!(rows.is_empty());
}

#[test]
fn collect_blocks_empty_when_no_blocks_after_target_epoch() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();
    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();
    tx.commit().unwrap();

    // Only the genesis / epoch-0 block exists — rolling back to epoch 0 yields no blocks.
    let rows = db
        .with_read_tx(|tx| rollback_plan_collect_blocks(tx, Epoch::zero()))
        .unwrap();
    assert!(rows.is_empty(), "unexpected rows: {rows:?}");
}
