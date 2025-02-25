//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

mod block_diffs {
    use tari_dan_common_types::{NumPreshards, ShardGroup, VersionedSubstateId};
    use tari_dan_storage::consensus_models::{BlockId, SubstateChange};
    use tari_engine_types::substate::Substate;
    use tari_transaction::TransactionId;

    use crate::helper::{build_substate_record, create_random_substate_id, create_rocksdb, create_sqlite};
    
    use super::*;

    #[test]
    fn block_diffs_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        block_diffs_operations(db);
    }

    #[test]
    fn block_diffs_rocksdb() {
        let db = create_rocksdb();
        block_diffs_operations(db);
    }

    fn block_diffs_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();
        
        // block_diffs_insert
        let block_id = BlockId::genesis();
        let substate_id = create_random_substate_id();
        let version = 0;
        let versioned_substate_id = VersionedSubstateId::new(substate_id.clone(), version);
        let substate_record = build_substate_record(&substate_id, version);
        let changes = SubstateChange::Up {
            id: versioned_substate_id,
            shard: ShardGroup::all_shards(NumPreshards::P4).start(),
            transaction_id: TransactionId::default(),
            substate: Substate::new(version, substate_record.substate_value.unwrap())
        };
        tx.block_diffs_insert(&block_id, &[changes]).unwrap();

        // block_diffs_get
        let res = tx.block_diffs_get(&block_id).unwrap();
        assert_eq!(res.changes.len(), 1);
    
        // block_diffs_get_last_change_for_substate
        // TODO: cannot be tested until "get_block_ids_with_commands_between" is implemented

        // block_diffs_remove
        tx.block_diffs_remove(&block_id).unwrap();
        let res = tx.block_diffs_get(&block_id).unwrap();
        assert_eq!(res.changes.len(), 0);

        tx.rollback().unwrap();
    }
}
