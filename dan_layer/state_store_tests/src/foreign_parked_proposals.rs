//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::FixedHash;
use tari_dan_common_types::{Epoch, ExtraData, NodeHeight, ShardGroup};
use tari_dan_storage::{
    consensus_models::{
        Block,
        BlockId,
        BlockPledge,
        ForeignParkedProposal,
        ForeignProposal,
        ForeignProposalStatus,
        QuorumCertificate,
        QuorumDecision,
    },
    StateStore,
    StateStoreWriteTransaction,
};
use tari_utilities::epoch_time::EpochTime;

use crate::{
    helper::{create_rocksdb, create_sqlite, transaction_id_from_seed},
    TEST_NUM_PRESHARDS,
};

#[test]
fn sqlite() {
    let db = create_sqlite();
    db.foreign_keys_off().unwrap();
    run_test(db);
}

#[test]
fn rocksdb() {
    let (db, _tmp) = create_rocksdb();
    run_test(db);
}

fn run_test(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    let zero_block = Block::zero_block(Default::default(), TEST_NUM_PRESHARDS);
    tx.blocks_insert(&zero_block).unwrap();
    zero_block.as_locked_block().set(&mut tx).unwrap();

    let fp = create_proposal(*zero_block.id());

    let tx_1 = transaction_id_from_seed(1);
    let tx_2 = transaction_id_from_seed(2);
    let tx_3 = transaction_id_from_seed(3);

    // Doesnt exist - no error
    let blocks = tx.foreign_parked_blocks_remove_all_by_transaction(&tx_1).unwrap();
    assert!(blocks.is_empty());

    let parked = ForeignParkedProposal::new(fp);
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

fn create_proposal(parent_id: BlockId) -> ForeignProposal {
    let qc1 = QuorumCertificate::new(
        *parent_id.hash(),
        parent_id,
        NodeHeight(1),
        Epoch(1),
        ShardGroup::all_shards(TEST_NUM_PRESHARDS),
        vec![],
        vec![],
        QuorumDecision::Accept,
    );

    let foreign_block = Block::create(
        Default::default(),
        parent_id,
        qc1.clone(),
        NodeHeight(2),
        Epoch(1),
        ShardGroup::all_shards(TEST_NUM_PRESHARDS),
        Default::default(),
        Default::default(),
        Default::default(),
        1,
        Default::default(),
        None,
        EpochTime::now().as_u64(),
        0,
        FixedHash::zero(),
        ExtraData::new(),
    )
    .unwrap();

    ForeignProposal {
        block: foreign_block.clone(),
        block_pledge: BlockPledge::new(),
        justify_qc: qc1,
        proposed_by_block: None,
        status: ForeignProposalStatus::New,
    }
}
