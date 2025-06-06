//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::Epoch;
use tari_ootle_storage::{
    consensus_models::{Block, BookkeepingModel, ForeignParkedProposal},
    StateStore,
    StateStoreWriteTransaction,
};

use crate::{
    helpers::{create_foreign_proposal, create_rocksdb, transaction_id_from_seed},
    TEST_NUM_PRESHARDS,
};

#[test]
fn rocksdb() {
    let (db, _tmp) = create_rocksdb();
    run_test(db);
}

fn run_test(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    let zero_block = Block::zero_block(Default::default(), TEST_NUM_PRESHARDS);
    tx.blocks_insert(&zero_block).unwrap();
    zero_block.as_locked().set(&mut tx).unwrap();

    let fp = create_foreign_proposal(*zero_block.id(), Epoch(1));

    let tx_1 = transaction_id_from_seed(1);
    let tx_2 = transaction_id_from_seed(2);
    let tx_3 = transaction_id_from_seed(3);

    // Doesnt exist - no error
    let blocks = tx.foreign_parked_blocks_remove_all_by_transaction(&tx_1).unwrap();
    assert!(blocks.is_empty());

    let (commit_proof, block_pledge) = fp.into_proposal().into_parts();
    let parked = ForeignParkedProposal::new(commit_proof, block_pledge);
    parked.insert(&mut tx).unwrap();
    parked.add_missing_transactions(&mut tx, &[tx_1, tx_2, tx_3]).unwrap();

    let blocks = tx.foreign_parked_blocks_remove_all_by_transaction(&tx_1).unwrap();
    assert!(blocks.is_empty());

    let blocks = tx.foreign_parked_blocks_remove_all_by_transaction(&tx_2).unwrap();
    assert!(blocks.is_empty());

    let blocks = tx.foreign_parked_blocks_remove_all_by_transaction(&tx_3).unwrap();
    assert_eq!(blocks.len(), 1);

    tx.rollback().unwrap();
}
