//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{NumPreshards, ShardGroup, VersionedSubstateId};
use tari_ootle_storage::{
    consensus_models::SubstatePledge,
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

use crate::helpers::{build_substate_value, create_random_substate_id, create_rocksdb, transaction_id_from_seed};

#[test]
fn foreign_substate_pledges_rocksdb() {
    let (db, _tmp) = create_rocksdb();
    foreign_substate_pledges_operations(db);
}

fn foreign_substate_pledges_operations(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    let transaction_id = transaction_id_from_seed(1);
    let transaction_id2 = transaction_id_from_seed(2);
    let shard_group = ShardGroup::all_shards(NumPreshards::P256);
    let pledge_1 = build_substate_pledge();
    let pledge_2 = build_substate_pledge();
    let pledge_3 = build_substate_pledge();

    tx.foreign_substate_pledges_save(&transaction_id, shard_group, &vec![pledge_1, pledge_2])
        .unwrap();
    tx.foreign_substate_pledges_save(&transaction_id2, shard_group, &vec![pledge_3])
        .unwrap();
    let res = tx
        .foreign_substate_pledges_get_all_by_transaction_id(&transaction_id)
        .unwrap();
    assert_eq!(res.len(), 2);
    let res = tx
        .foreign_substate_pledges_get_all_by_transaction_id(&transaction_id2)
        .unwrap();
    assert_eq!(res.len(), 1);

    tx.foreign_substate_pledges_remove_many(vec![&transaction_id]).unwrap();
    let res = tx
        .foreign_substate_pledges_get_all_by_transaction_id(&transaction_id)
        .unwrap();
    assert_eq!(res.len(), 0);

    // Doesnt exist - doesnt error
    tx.foreign_substate_pledges_remove_many(vec![&transaction_id_from_seed(123)])
        .unwrap();

    tx.rollback().unwrap();
}

fn build_substate_pledge() -> SubstatePledge {
    SubstatePledge::Input {
        substate_id: VersionedSubstateId::new(create_random_substate_id(), 0),
        is_write: false,
        substate: Box::new(build_substate_value(None)),
    }
}
