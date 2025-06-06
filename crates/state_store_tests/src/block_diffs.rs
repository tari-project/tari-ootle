//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::substate::{hash_substate, Substate};
use tari_ootle_common_types::VersionedSubstateId;
use tari_ootle_storage::{
    consensus_models::SubstateChange,
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

use crate::helpers::{
    build_substate_record,
    build_substate_value,
    commit_chain,
    create_chain,
    create_random_substate_id,
    create_rocksdb,
};

#[test]
fn block_diffs_rocksdb() {
    let (db, _tmp) = create_rocksdb();
    block_diffs_operations(db);
}

fn block_diffs_operations(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);

    // block_diffs_insert
    let block8 = chain[8].clone();
    let block_id8 = *block8.id();
    let block9 = chain[9].clone();
    let block_id9 = *block9.id();
    let substate_id = create_random_substate_id();
    let version = 0;
    let substate_record = build_substate_record(&substate_id, version);
    let change = SubstateChange::Up {
        id: substate_id.clone(),
        shard: block9.shard_group().start(),
        substate: Box::new(Substate::new(version, substate_record.substate_value.clone().unwrap())),
    };
    tx.block_diffs_insert(&block_id8, &[change]).unwrap();
    let value2 = build_substate_value(Some(
        substate_record.substate_value().unwrap().component().unwrap().entity_id,
    ));
    let versioned_substate_id = VersionedSubstateId::new(substate_id.clone(), version);
    let changes = &[
        SubstateChange::Down {
            id: versioned_substate_id.clone(),
            shard: block9.shard_group().end(),
        },
        SubstateChange::Up {
            id: substate_id.clone(),
            shard: block9.shard_group().end(),
            substate: Box::new(Substate::new(version + 1, value2.clone())),
        },
    ];
    tx.block_diffs_insert(&block_id9, changes).unwrap();

    // block_diffs_get
    let res = tx.block_diffs_get(&block_id9).unwrap();
    assert_eq!(res.changes.len(), 2);

    let change = tx
        .block_diffs_get_last_change_for_substate(&block_id9, &substate_id)
        .unwrap();
    match &change {
        SubstateChange::Up { id, shard, substate } => {
            assert_eq!(id, versioned_substate_id.substate_id());
            assert_eq!(*shard, block9.shard_group().end());
            assert_eq!(substate.version(), version + 1);
            assert_eq!(substate.to_value_hash(), hash_substate(&value2, version + 1));
        },
        SubstateChange::Down { .. } => panic!("Expected SubstateChange::Up but got {change}"),
    }

    // block_diffs_remove
    tx.block_diffs_remove(&block_id9).unwrap();
    let res = tx.block_diffs_get(&block_id9).unwrap();
    assert_eq!(res.changes.len(), 0);

    tx.rollback().unwrap();
}
