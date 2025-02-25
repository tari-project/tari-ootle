//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

mod last_inserted {
    use tari_dan_common_types::{NumPreshards, ShardGroup, VersionedSubstateId};
    use tari_dan_storage::consensus_models::SubstatePledge;
    use tari_transaction::TransactionId;

    use crate::helper::{build_substate_value, create_random_substate_id, create_rocksdb, create_sqlite};
    
    use super::*;

    #[test]
    fn foreign_substate_pledges_sqlite() {
        let db = create_sqlite();
        db.foreign_keys_off().unwrap();
        foreign_substate_pledges_operations(db);
    }

    #[test]
    fn foreign_substate_pledges_rocksdb() {
        let db = create_rocksdb();
        foreign_substate_pledges_operations(db);
    }

    fn foreign_substate_pledges_operations(db: impl StateStore) {
        let mut tx = db.create_write_tx().unwrap();

        let transaction_id = TransactionId::default();
        let shard_group = ShardGroup::all_shards(NumPreshards::P1);
        let pledge_1 = build_substate_pledge();
        let pledge_2 = build_substate_pledge();

        tx.foreign_substate_pledges_save(&transaction_id, shard_group, &vec![pledge_1.clone(), pledge_2]).unwrap();
        let res = tx.foreign_substate_pledges_get_all_by_transaction_id(&transaction_id).unwrap();
        assert_eq!(res.len(), 2);

        tx.foreign_substate_pledges_remove_many(vec![&transaction_id]).unwrap();
        let res = tx.foreign_substate_pledges_get_all_by_transaction_id(&transaction_id).unwrap();
        assert_eq!(res.len(), 0);

        tx.rollback().unwrap();
    }

    fn build_substate_pledge () -> SubstatePledge {
        SubstatePledge::Input {
            substate_id: VersionedSubstateId::new(create_random_substate_id(), 0),
            is_write: false,
            substate: build_substate_value(None)
        }
    }
}
