//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

mod last_inserted {
    use tari_dan_storage::consensus_models::{BlockId, LastVoted};

    use crate::helper::{assert_eq_debug, create_rocksdb, create_sqlite};
    
    use super::*;

    #[test]
    fn last_inserted_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        last_inserted_operations(db);
    }

    #[test]
    fn last_inserted_rocksdb() {
        let db = create_rocksdb();
        last_inserted_operations(db);
    }

    fn last_inserted_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        // last voted
        let mut last_voted = LastVoted {
            block_id: BlockId::genesis(),
            height: NodeHeight(123),
            epoch: Epoch::zero(),
        };
        tx.last_voted_set(&last_voted).unwrap();
        let res = tx.last_voted_get().unwrap();
        assert_eq_debug(&res, &last_voted);

        last_voted.epoch = last_voted.epoch + Epoch(1);

        tx.last_voted_set(&last_voted).unwrap();
        let res = tx.last_voted_get().unwrap();
        assert_eq_debug(&res, &last_voted);

        tx.rollback().unwrap();
    }

}
