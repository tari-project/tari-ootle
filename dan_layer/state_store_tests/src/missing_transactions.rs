//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::FixedHash;
use tari_dan_common_types::{Epoch, ExtraData, NodeHeight, NumPreshards, ShardGroup};
use tari_dan_storage::{
    consensus_models::{Block, Command},
    StateStore,
    StateStoreWriteTransaction,
};
use tari_template_lib::prelude::SchnorrSignatureBytes;
use tari_utilities::epoch_time::EpochTime;

use crate::helpers::{create_block, create_rocksdb, create_tx_atom};

#[test]
fn missing_transactions_rocksdb() {
    let (db, _tmp) = create_rocksdb();
    missing_transactions_operations(db);
}

fn missing_transactions_operations(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    let network = Default::default();

    // add some blocks to the database
    let genesis = create_block(None);
    genesis.insert(&mut tx).unwrap();

    let atom1 = create_tx_atom();
    let block1 = Block::create(
        network,
        *genesis.id(),
        genesis.justify().clone(),
        None,
        NodeHeight(1),
        Epoch(0),
        ShardGroup::all_shards(NumPreshards::P64),
        Default::default(),
        // Need to have a command in, otherwise this block will not be included internally in the query because it
        // cannot cause a state change without any commands
        [Command::LocalPrepare(atom1.clone())].into_iter().collect(),
        Default::default(),
        Default::default(),
        SchnorrSignatureBytes::zero(),
        EpochTime::now().as_u64(),
        FixedHash::zero(),
        ExtraData::default(),
    )
    .unwrap();

    // missing_transactions_insert
    let missing_transaction_ids = vec![&atom1.id];
    tx.parked_block_insert(&block1, &[], missing_transaction_ids).unwrap();

    // blocks_get_pending_transactions
    // let res = tx.blocks_get_pending_transactions(block1.id()).unwrap();
    // assert_eq!(res.len(), 1);
    // assert_eq!(res[0], atom1.id);

    // missing_transactions_remove
    tx.parked_block_remove_missing_transaction(block1.height(), atom1.id())
        .unwrap();
    // let res = tx.blocks_get_pending_transactions(block1.id()).unwrap();
    // assert_eq!(res.len(), 0);

    tx.rollback().unwrap();
}
