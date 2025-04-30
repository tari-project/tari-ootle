//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::FixedHash;
use tari_dan_common_types::{Epoch, ExtraData, NodeHeight, NumPreshards, ShardGroup};
use tari_dan_storage::{
    consensus_models::{Block, Command, ForeignProposalStatus},
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_template_lib::prelude::SchnorrSignatureBytes;
use tari_utilities::epoch_time::EpochTime;

use crate::helpers::{assert_eq_debug, create_foreign_proposal, create_random_block_id, create_rocksdb, create_sqlite};

#[ignore = "some issue with the QcId"]
#[test]
fn foreign_proposals_sqlite() {
    let db = create_sqlite();
    db.foreign_keys_off().unwrap();
    foreign_proposals_operations(db);
}

#[test]
fn foreign_proposals_rocksdb() {
    let (db, _tmp) = create_rocksdb();
    foreign_proposals_operations(db);
}

#[allow(clippy::too_many_lines)]
fn foreign_proposals_operations(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    let network = Default::default();
    const EPOCH: Epoch = Epoch(2);

    let zero_block = Block::zero_block(network, NumPreshards::P64);
    tx.blocks_insert(&zero_block).unwrap();
    zero_block.as_locked_block().set(&mut tx).unwrap();
    let proposal1 = create_foreign_proposal(*zero_block.id(), EPOCH);
    tx.foreign_proposals_save(&proposal1).unwrap();

    let block1 = Block::create(
        network,
        *zero_block.id(),
        zero_block.justify().clone(),
        NodeHeight(2),
        EPOCH,
        ShardGroup::all_shards(NumPreshards::P64),
        Default::default(),
        [Command::ForeignProposal(proposal1.to_atom())]
            .iter()
            .cloned()
            .collect(),
        Default::default(),
        2,
        SchnorrSignatureBytes::zero(),
        EpochTime::now().as_u64(),
        FixedHash::zero(),
        ExtraData::new(),
    )
    .unwrap();
    block1.as_locked_block().set(&mut tx).unwrap();

    tx.blocks_insert(&block1).unwrap();
    tx.quorum_certificates_insert(block1.justify()).unwrap();
    let fork_block = Block::create(
        network,
        *zero_block.id(),
        zero_block.justify().clone(),
        NodeHeight(2),
        EPOCH,
        ShardGroup::all_shards(NumPreshards::P64),
        Default::default(),
        Default::default(),
        Default::default(),
        5,
        SchnorrSignatureBytes::zero(),
        EpochTime::now().as_u64(),
        FixedHash::zero(),
        ExtraData::new(),
    )
    .unwrap();
    tx.blocks_insert(&fork_block).unwrap();

    // foreign_proposals_get_any
    let res = tx.foreign_proposals_get_any([proposal1.block_id()]).unwrap();
    assert_eq!(res.len(), 1);
    assert_eq_debug(&res[0], &proposal1);

    // foreign_proposals_exists
    let res = tx.foreign_proposals_exists(proposal1.block_id()).unwrap();
    assert!(res);
    let res = tx.foreign_proposals_exists(&create_random_block_id()).unwrap();
    assert!(!res);

    // foreign_proposals_get_all_new
    tx.foreign_proposals_set_status(
        proposal1.block_id(),
        ForeignProposalStatus::Proposed,
        Some(&block1.as_leaf_block()),
    )
    .unwrap();
    let res = tx.foreign_proposals_get_all_new(block1.id(), 10).unwrap();
    assert_eq!(res.len(), 0);
    let res = tx.foreign_proposals_get_all_new(fork_block.id(), 10).unwrap();
    assert_eq!(res.len(), 1);
    assert_eq_debug(res[0].proposal(), proposal1.proposal());
    assert!(res[0].status().is_proposed());

    // foreign_proposal_get_all_pending
    // let res = tx.foreign_proposal_get_all_pending(block.id(), block.id()).unwrap();
    // assert_eq!(res.len(), 1);

    // foreign_proposals_has_unconfirmed
    let res = tx.foreign_proposals_has_unconfirmed(Epoch(4)).unwrap();
    assert!(res);
    let res = tx.foreign_proposals_has_unconfirmed(Epoch(0)).unwrap();
    assert!(!res);

    // foreign_proposals_set_status
    let updated_status = ForeignProposalStatus::Confirmed;
    tx.foreign_proposals_set_status(proposal1.block_id(), updated_status, Some(&block1.as_leaf_block()))
        .unwrap();
    let res = tx.foreign_proposals_get_any(vec![proposal1.block_id()]).unwrap();
    let confirmed_proposal = res[0].clone();
    assert_eq!(confirmed_proposal.status(), updated_status);

    // foreign_proposals_delete
    tx.foreign_proposals_delete(proposal1.block_id()).unwrap();
    let res = tx.foreign_proposals_exists(proposal1.block_id()).unwrap();
    assert!(!res);

    tx.rollback().unwrap();
}
