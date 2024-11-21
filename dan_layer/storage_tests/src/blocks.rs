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

        tx.rollback().unwrap();
    }
}