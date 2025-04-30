//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use tari_dan_common_types::{shard::Shard, Epoch, NodeHeight, VersionedSubstateId, VersionedSubstateIdRef};
use tari_dan_storage::{
    consensus_models::{Block, QcId},
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_engine_types::substate::SubstateId;
use tari_template_lib::{models::ComponentAddress, types::ObjectKey};

use crate::{
    helpers::{assert_eq_debug, build_substate_record, create_rocksdb, create_sqlite},
    TEST_NUM_PRESHARDS,
};

fn substate_id(seed: u8) -> SubstateId {
    let address = ComponentAddress::from_array([seed; ObjectKey::LENGTH]);
    SubstateId::Component(address)
}

#[test]
fn sqlite() {
    let db = create_sqlite();
    db.foreign_keys_off().unwrap();
    operations(db);
}

#[test]
fn rocksdb() {
    let (db, _tmp) = create_rocksdb();
    operations(db);
}

fn operations(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    let zero_block = Block::zero_block(Default::default(), TEST_NUM_PRESHARDS);
    zero_block.insert(&mut tx).unwrap();

    // substate 1
    let substate1_id = substate_id(1);
    let mut substate1 = build_substate_record(&substate1_id, 0);
    substate1.created_block = *zero_block.id();
    let substate1_address = substate1.to_substate_address();
    tx.substates_create(&substate1).unwrap();
    tx.substates_down(
        VersionedSubstateId::new(substate1_id.clone(), 0),
        Shard::first(),
        Epoch(123),
        NodeHeight(123),
        &QcId::zero(),
    )
    .unwrap();
    // substate 1 (version 1)
    let mut substate1b = build_substate_record(&substate1_id, 1);
    substate1b.created_block = *zero_block.id();
    let substate1b_address = substate1b.to_substate_address();
    tx.substates_create(&substate1b).unwrap();

    // substate 2
    let substate2_id = substate_id(2);
    let mut substate2 = build_substate_record(&substate2_id, 0);
    substate2.created_block = *zero_block.id();
    let substate2_address = substate2.to_substate_address();
    tx.substates_create(&substate2).unwrap();

    // check that we can get all the newly inserted substates
    let res = tx.substates_get(&substate1_address).unwrap();
    assert_eq_debug(&res.substate_value, &substate1.substate_value);

    let res = tx.substates_get(&substate1b_address).unwrap();
    assert_eq_debug(&res, &substate1b);

    let res = tx.substates_get(&substate2_address).unwrap();
    assert_eq_debug(&res, &substate2);

    // substates_get_any fetches all substates
    let req = [
        VersionedSubstateIdRef::new(&substate1_id, 0),
        VersionedSubstateIdRef::new(&substate2_id, 0),
    ];
    let res = tx.substates_get_any(&req).unwrap();
    assert_eq!(res.len(), 2);

    // substates_get_any fetches the last version of a substate
    let mut req = HashSet::new();
    req.insert(VersionedSubstateIdRef::new(&substate1_id, 0));
    let res = tx.substates_get_any(&req).unwrap();
    assert_eq!(res.len(), 1);
    // Historical value
    assert!(res[0].substate_value.is_some());
    assert_eq!(res[0].state_hash(), substate1.state_hash());
    assert_eq_debug(
        res[0].substate_value.as_ref().unwrap(),
        substate1.substate_value.as_ref().unwrap(),
    );

    // substates_get_any_max_version
    let substate_ids = vec![substate1_id.clone(), substate2_id.clone()];
    let res = tx.substates_get_any_max_version(&substate_ids).unwrap();
    assert_eq!(res.len(), 2);
    assert!(res.iter().any(|s| s.substate_id == substate1_id && s.version == 1));
    assert!(res.iter().any(|s| s.substate_id == substate2_id && s.version == 0));

    // substates_get_max_version_for_substate
    let res = tx.substates_get_max_version_for_substate(&substate1_id).unwrap();
    assert_eq!(res, (1, true));
    let res = tx.substates_get_max_version_for_substate(&substate2_id).unwrap();
    assert_eq!(res, (0, true));

    // substates_any_exist (all exist)
    let substate_ids = [
        VersionedSubstateId::new(substate1_id.clone(), 0),
        VersionedSubstateId::new(substate2_id.clone(), 0),
    ];
    let res = tx
        .substates_any_exist(substate_ids.iter().map(|id| id.as_ref()))
        .unwrap();
    assert!(res);

    // substates_any_exist (some do not exist)
    let substate_ids = [
        VersionedSubstateId::new(substate1_id.clone(), 100), // version should not exist
        VersionedSubstateId::new(substate2_id.clone(), 0),
    ];
    let res = tx
        .substates_any_exist(substate_ids.iter().map(|id| id.as_ref()))
        .unwrap();
    assert!(res);

    // substates_any_exist (none exist)
    let substate_ids = [
        VersionedSubstateId::new(substate1_id, 100), // version should not exist
        VersionedSubstateId::new(substate2_id, 100), // version should not exist
    ];
    let res = tx
        .substates_any_exist(substate_ids.iter().map(|id| id.as_ref()))
        .unwrap();
    assert!(!res);

    // substates_down
    let res = tx.substates_get(&substate2_address).unwrap();
    assert!(res.destroyed.is_none());

    let versioned_substate_id = VersionedSubstateId::new(substate2.substate_id, substate2.version);
    let shard = Shard::first();
    let epoch = Epoch::zero();
    let destroyed_block_height = NodeHeight::zero();
    let destroyed_qc_id = QcId::zero();

    tx.substates_down(
        versioned_substate_id,
        shard,
        epoch,
        destroyed_block_height,
        &destroyed_qc_id,
    )
    .unwrap();
    let res = tx.substates_get(&substate2_address).unwrap();
    assert!(res.destroyed.is_some());

    tx.rollback().unwrap();
}
