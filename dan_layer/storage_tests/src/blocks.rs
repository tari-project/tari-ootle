//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::{rngs::OsRng, RngCore};
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::{
    consensus_models::{Block, Command, Decision, TransactionAtom, TransactionPoolStage, TransactionPoolStatusUpdate},
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_utilities::epoch_time::EpochTime;


mod basic_block_operations {
    use tari_dan_common_types::{ExtraData, NumPreshards, ShardGroup};

    use crate::util::{create_rocksdb, create_sqlite, create_tx_atom};

    use super::*;

    // TODO: sqlite fails due to missing foreign key values
    #[ignore]
    #[test]
    fn basic_block_operations_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        basic_block_operations(db);
    }

    #[test]
    fn basic_block_operations_rocksdb() {
        let db = create_rocksdb();
        basic_block_operations(db);
    }

    fn basic_block_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // insert multiple blocks
        let network = Default::default();
        let atom1 = create_tx_atom();
        
        let zero_block = Block::zero_block(network, NumPreshards::P64);
        zero_block.insert(&mut tx).unwrap();

        let block1 = Block::create(
            network,
            *zero_block.id(),
            zero_block.justify().clone(),
            NodeHeight(1),
            Epoch(0),
            ShardGroup::all_shards(NumPreshards::P64),
            Default::default(),
            // Need to have a command in, otherwise this block will not be included internally in the query because it
            // cannot cause a state change without any commands
            [Command::Prepare(atom1.clone())].into_iter().collect(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            EpochTime::now().as_u64(),
            0,
            FixedHash::zero(),
            ExtraData::default(),
        )
        .unwrap();
        block1.insert(&mut tx).unwrap();

        // fetch blocks by id
        let res = tx.blocks_get(zero_block.id()).unwrap();
        assert_eq!(res.to_string(), zero_block.to_string());
        let res = tx.blocks_get(block1.id()).unwrap();
        assert_eq!(res.to_string(), block1.to_string());

        // blocks exist method
        assert!(tx.blocks_exists(zero_block.id()).unwrap());
        assert!(tx.blocks_exists(block1.id()).unwrap());

        // get all blocks
        let res = tx.blocks_get_count().unwrap();
        assert_eq!(res, 2);

        // set is_justified flag
        let block1_from_db = tx.blocks_get(block1.id()).unwrap();
        assert_eq!(block1_from_db.is_justified(), false);
        tx.blocks_set_flags(block1_from_db.id(), None, Some(true)).unwrap();
        let block1_from_db = tx.blocks_get(block1.id()).unwrap();
        assert_eq!(block1_from_db.is_justified(), true);
        
        // set is_commited flag
        let block1_from_db = tx.blocks_get(block1.id()).unwrap();
        assert_eq!(block1_from_db.is_committed(), false);
        tx.blocks_set_flags(block1_from_db.id(), Some(true), None).unwrap();
        let block1_from_db = tx.blocks_get(block1.id()).unwrap();
        assert_eq!(block1_from_db.is_committed(), true);

        // delete one of the blocks
        tx.blocks_delete(block1.id()).unwrap();
        let res = tx.blocks_get_count().unwrap();
        assert_eq!(res, 1);
        assert!(tx.blocks_get(block1.id()).is_err());
        assert!(!tx.blocks_exists(block1.id()).unwrap());

        tx.rollback().unwrap();
    }
}

mod block_parent_operations {
    use tari_dan_common_types::{ExtraData, NumPreshards, ShardGroup};

    use crate::util::{create_rocksdb, create_sqlite, create_tx_atom};

    use super::*;

    // TODO: sqlite fails due to missing foreign key values
    #[ignore]
    #[test]
    fn block_parent_operations_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        block_parent_operations(db);
    }

    #[test]
    fn block_parent_operations_rocksdb() {
        let db = create_rocksdb();
        block_parent_operations(db);
    }

    fn block_parent_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // insert multiple blocks
        let network = Default::default();
        let atom1 = create_tx_atom();
        let atom2 = create_tx_atom();
        
        let zero_block = Block::zero_block(network, NumPreshards::P64);
        zero_block.insert(&mut tx).unwrap();

        let block1 = Block::create(
            network,
            *zero_block.id(),
            zero_block.justify().clone(),
            NodeHeight(1),
            Epoch(0),
            ShardGroup::all_shards(NumPreshards::P64),
            Default::default(),
            // Need to have a command in, otherwise this block will not be included internally in the query because it
            // cannot cause a state change without any commands
            [Command::Prepare(atom1.clone())].into_iter().collect(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            EpochTime::now().as_u64(),
            0,
            FixedHash::zero(),
            ExtraData::default(),
        )
        .unwrap();
        block1.insert(&mut tx).unwrap();

        let block2 = Block::create(
            network,
            *block1.id(),
            block1.justify().clone(),
            NodeHeight(1),
            Epoch(0),
            ShardGroup::all_shards(NumPreshards::P64),
            Default::default(),
            // Need to have a command in, otherwise this block will not be included internally in the query because it
            // cannot cause a state change without any commands
            [Command::Prepare(atom2.clone())].into_iter().collect(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            EpochTime::now().as_u64(),
            0,
            FixedHash::zero(),
            ExtraData::default(),
        )
        .unwrap();
        block2.insert(&mut tx).unwrap();

        // check that all blocks are inserted
        let res = tx.blocks_get(zero_block.id()).unwrap();
        assert_eq!(res.to_string(), zero_block.to_string());
        let res = tx.blocks_get(block1.id()).unwrap();
        assert_eq!(res.to_string(), block1.to_string());
        let res = tx.blocks_get(block2.id()).unwrap();
        assert_eq!(res.to_string(), block2.to_string());

        // blocks_is_ancestor
        let res = tx.blocks_is_ancestor(block2.id(), block1.id()).unwrap();
        assert!(res);
        let res = tx.blocks_is_ancestor(block2.id(), zero_block.id()).unwrap();
        assert!(res);
        let res = tx.blocks_is_ancestor(block1.id(), zero_block.id()).unwrap();
        assert!(res);
        let res = tx.blocks_is_ancestor(block1.id(), block2.id()).unwrap();
        assert!(!res);
        let res = tx.blocks_is_ancestor(block2.id(), block2.id()).unwrap();
        assert!(!res);

        // blocks_get_ids_by_parent
        let res = tx.blocks_get_ids_by_parent(zero_block.id()).unwrap();
        assert_eq!(res, vec![*block1.id()]);
        let res = tx.blocks_get_ids_by_parent(block1.id()).unwrap();
        assert_eq!(res, vec![*block2.id()]);
        let res = tx.blocks_get_ids_by_parent(block2.id()).unwrap();
        assert_eq!(res, vec![]);

        // blocks_get_all_by_parent
        let res = tx.blocks_get_all_by_parent(zero_block.id()).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].to_string(), block1.to_string());
        let res = tx.blocks_get_all_by_parent(block1.id()).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].to_string(), block2.to_string());
        let res = tx.blocks_get_all_by_parent(block2.id()).unwrap();
        assert_eq!(res.len(), 0);

        // blocks_get_parent_chain
        let res = tx.blocks_get_parent_chain(zero_block.id(), 10).unwrap();
        assert_eq!(res.len(), 0);
        let res = tx.blocks_get_parent_chain(block1.id(), 10).unwrap();
        assert_eq!(res.len(), 0);
        let res = tx.blocks_get_parent_chain(block2.id(), 10).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].to_string(), block1.to_string());
        // TODO: test a longer parent chain

        // TODO: have a block with multiple children and check method results
        // TODO: remove block1 and check method results

        tx.rollback().unwrap();
    }
}


mod block_query_operations {
    use tari_dan_common_types::{ExtraData, ExtraFieldKey, NumPreshards, ShardGroup};

    use crate::util::{create_rocksdb, create_sqlite, create_tx_atom};

    use super::*;

    // TODO: sqlite fails due to missing foreign key values
    #[ignore]
    #[test]
    fn block_query_operations_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        block_query_operations(db);
    }

    #[test]
    fn block_query_operations_rocksdb() {
        let db = create_rocksdb();
        block_query_operations(db);
    }

    fn block_query_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // insert multiple blocks
        let network = Default::default();
        let atom1 = create_tx_atom();
        let atom2 = create_tx_atom();
        
        let zero_block = Block::zero_block(network, NumPreshards::P64);
        zero_block.insert(&mut tx).unwrap();

        let block1 = Block::create(
            network,
            *zero_block.id(),
            zero_block.justify().clone(),
            NodeHeight(1),
            Epoch(0),
            ShardGroup::all_shards(NumPreshards::P64),
            Default::default(),
            // Need to have a command in, otherwise this block will not be included internally in the query because it
            // cannot cause a state change without any commands
            [Command::Prepare(atom1.clone())].into_iter().collect(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            EpochTime::now().as_u64(),
            0,
            FixedHash::zero(),
            ExtraData::default(),
        )
        .unwrap();
        block1.insert(&mut tx).unwrap();

        let block2 = Block::create(
            network,
            *block1.id(),
            block1.justify().clone(),
            NodeHeight(2),
            Epoch(0),
            ShardGroup::all_shards(NumPreshards::P64),
            Default::default(),
            // Need to have a command in, otherwise this block will not be included internally in the query because it
            // cannot cause a state change without any commands
            [Command::Prepare(atom2.clone())].into_iter().collect(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            EpochTime::now().as_u64(),
            0,
            FixedHash::zero(),
            ExtraData::default(),
        )
        .unwrap();
        block2.insert(&mut tx).unwrap();

        let mut block3_data = ExtraData::new();
        block3_data
            .insert(ExtraFieldKey::SidechainId, "block3".as_bytes().to_vec().try_into().unwrap());
        let block3 = Block::create(
            network,
            *block1.id(),
            block1.justify().clone(),
            NodeHeight(2),
            Epoch(0),
            ShardGroup::all_shards(NumPreshards::P64),
            Default::default(),
            // Need to have a command in, otherwise this block will not be included internally in the query because it
            // cannot cause a state change without any commands
            [Command::Prepare(atom2.clone())].into_iter().collect(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            EpochTime::now().as_u64(),
            0,
            FixedHash::zero(),
            block3_data.clone(),
        )
        .unwrap();
        block3.insert(&mut tx).unwrap();

        // blocks_get_all_ids_by_height
        let res = tx.blocks_get_all_ids_by_height(Epoch(0), NodeHeight(1)).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], *block1.id());
        let res = tx.blocks_get_all_ids_by_height(Epoch(0), NodeHeight(2)).unwrap();
        assert_eq!(res.len(), 2);

        // TODO: blocks_get_genesis_for_epoch
        // TODO: blocks_get_last_n_in_epoch
        // TODO: blocks_get_all_between

        // TODO: blocks_get_pending_transactions
        // TODO: blocks_get_total_leader_fee_for_epoch
        // TODO: blocks_get_any_with_epoch_range
        // TODO: blocks_get_paginated
        // TODO: filtered_blocks_get_count

        tx.rollback().unwrap();
    }
}