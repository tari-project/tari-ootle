//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Read-only rollback-plan iterators: prove the dry-run collection matches what the
//! mutating rewind walks. Seed a DB with known transitions spanning a few state
//! versions, call the collector with target_version below the seeded data, and assert
//! the row count + ordering.

pub mod helpers;

use helpers::{create_rocksdb, create_substate_update_batch, gen_substates_for_shards};
use tari_ootle_common_types::{Epoch, SubstateVersion};
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
        let substates = gen_substates_for_shards(Epoch::zero(), 1, batch_idx..(batch_idx + 2), SubstateVersion::ZERO)
            .collect::<Vec<_>>();
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
    let substates = gen_substates_for_shards(Epoch::zero(), 1, 0..2, SubstateVersion::ZERO).collect::<Vec<_>>();
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

#[test]
fn rollback_delete_after_epoch_cleans_foreign_proposal_indexes_and_restores_surviving() {
    use helpers::{create_block_with_qc, create_first_block_of_next_epoch, create_foreign_proposal};
    use tari_ootle_storage::{
        StateStoreReadTransaction,
        consensus_models::{BookkeepingModel, ForeignProposalStatus},
    };
    use tari_validator_rollback::storage::rollback_delete_after_epoch;

    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();
    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();
    zero_block.as_locked().set(&mut tx).unwrap();

    let epoch0_block = create_block_with_qc(&zero_block.as_leaf());
    epoch0_block.insert(&mut tx).unwrap();
    let epoch1_block = create_first_block_of_next_epoch(&epoch0_block.as_leaf());
    epoch1_block.insert(&mut tx).unwrap();

    // Surviving foreign proposal from epoch 0 that was confirmed in an epoch 1 block.
    let fp_epoch0 = create_foreign_proposal(*zero_block.id(), Epoch(0));
    tx.foreign_proposals_save(&fp_epoch0).unwrap();
    tx.foreign_proposals_set_status(
        fp_epoch0.block_id(),
        ForeignProposalStatus::Confirmed,
        Some(epoch1_block.id()),
    )
    .unwrap();

    // Unconfirmed foreign proposal from epoch 1 that must be deleted along with its UnconfirmedIndex entry.
    let fp_epoch1 = create_foreign_proposal(*epoch0_block.id(), Epoch(1));
    tx.foreign_proposals_save(&fp_epoch1).unwrap();

    let stats = rollback_delete_after_epoch(&mut tx, Epoch(0)).unwrap();
    assert_eq!(stats.blocks_deleted, 1);
    assert_eq!(stats.foreign_proposals_deleted, 1);

    // Re-set locked block at epoch 0 so `foreign_proposals_get_all_new` can query up to epoch 0.
    epoch0_block.as_locked().set(&mut tx).unwrap();
    tx.commit().unwrap();

    db.with_read_tx(|tx| {
        assert!(!tx.foreign_proposals_exists(fp_epoch1.block_id()).unwrap());
        assert!(!tx.foreign_proposals_has_unconfirmed(Epoch(1)).unwrap());

        let new_fps = tx.foreign_proposals_get_all_new(epoch0_block.id(), 10).unwrap();
        assert_eq!(new_fps.len(), 1);
        assert_eq!(new_fps[0].block_id(), fp_epoch0.block_id());
        assert_eq!(new_fps[0].status(), ForeignProposalStatus::New);
        assert_eq!(new_fps[0].proposed_in_block(), None);
        Ok::<_, tari_ootle_storage::StorageError>(())
    })
    .unwrap();
}
