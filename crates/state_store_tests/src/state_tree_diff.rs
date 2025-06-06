//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::QcId;
use tari_ootle_storage::{
    consensus_models::{BookkeepingModel, PendingShardStateTreeDiff},
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_state_tree::StateHashTreeDiff;

use crate::helpers::{create_block, create_rocksdb};

#[test]
fn pending_state_tree_diff_rocksdb() {
    let (db, _tmp) = create_rocksdb();
    pending_state_tree_diff_operations(db);
}

fn pending_state_tree_diff_operations(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    // add some (committed) blocks to the database
    let mut genesis = create_block(None);
    genesis.set_commit_qc(QcId::zero());
    genesis.insert(&mut tx).unwrap();
    tx.blocks_set_qcs(genesis.id(), Some(&QcId::zero()), Some(&QcId::zero()))
        .unwrap();
    tx.proposal_certificates_save(genesis.justify()).unwrap();
    genesis.as_locked().set(&mut tx).unwrap();
    genesis.as_leaf().set(&mut tx).unwrap();

    let mut block_1 = create_block(Some(&genesis));
    block_1.set_commit_qc(QcId::zero());
    block_1.insert(&mut tx).unwrap();

    let block_2 = create_block(Some(&block_1));
    block_2.insert(&mut tx).unwrap();

    let block_3 = create_block(Some(&block_2));
    block_3.insert(&mut tx).unwrap();

    // pending_state_tree_diffs_insert
    let shard = block_2.shard_group().shard_iter().next().unwrap();
    let diff = PendingShardStateTreeDiff::new(0, StateHashTreeDiff::new());
    tx.pending_state_tree_diffs_insert(*block_2.id(), shard, &diff).unwrap();

    // pending_state_tree_diffs_get_all_up_to_commit_block
    let res = tx
        .pending_state_tree_diffs_get_all_up_to_commit_block(block_3.id())
        .unwrap();
    assert_eq!(res.len(), 1);

    // pending_state_tree_diffs_remove_and_return_by_block
    let res = tx
        .pending_state_tree_diffs_remove_and_return_by_block(block_2.id())
        .unwrap();
    assert_eq!(res.len(), 1);
    let res = tx
        .pending_state_tree_diffs_get_all_up_to_commit_block(block_3.id())
        .unwrap();
    assert_eq!(res.len(), 0);

    tx.rollback().unwrap();
}
