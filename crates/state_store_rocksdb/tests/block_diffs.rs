//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::substate::{Substate, hash_substate};
use tari_ootle_common_types::{SubstateVersion, VersionedSubstateId, optional::Optional};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{Block, SubstateChange},
};
use tari_state_store_rocksdb::{DatabaseOptions, RocksDbStateStore};

pub mod helpers;
use helpers::{
    build_substate_record,
    build_substate_value,
    commit_chain,
    create_block_with_qc,
    create_chain,
    create_random_substate_id,
    create_rocksdb,
};
use tari_engine_types::Epoch;

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
    let version = SubstateVersion::ZERO;
    let substate_record = build_substate_record(&substate_id, version, 1);
    let change = SubstateChange::Up {
        id: substate_id.clone(),
        shard: block9.shard_group().start(),
        substate: Box::new(Substate::new(version, substate_record.substate_value.clone().unwrap())),
    };
    tx.block_diffs_insert(&block_id8, &[change]).unwrap();
    let value2 = build_substate_value(Some(
        *substate_record
            .substate_value()
            .unwrap()
            .component()
            .unwrap()
            .entity_id(),
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
            substate: Box::new(Substate::new(version.next(), value2.clone())),
        },
    ];
    tx.block_diffs_insert(&block_id9, changes).unwrap();

    // block_diffs_get
    let res = tx.block_diffs_get(&block_id9).unwrap();
    assert_eq!(res.changes().len(), 2);

    let change = tx
        .block_diffs_get_last_change_for_substate(&block_id9, &substate_id)
        .unwrap();
    match &change {
        SubstateChange::Up { id, shard, substate } => {
            assert_eq!(id, versioned_substate_id.substate_id());
            assert_eq!(*shard, block9.shard_group().end());
            assert_eq!(substate.version(), version.next());
            let at_epoch = Epoch::zero();
            assert_eq!(
                substate.to_value_hash(helpers::NETWORK, at_epoch),
                hash_substate(helpers::NETWORK, &value2, version.next(), at_epoch)
            );
        },
        SubstateChange::Down { .. } => panic!("Expected SubstateChange::Up but got {change}"),
    }

    // block_diffs_remove
    tx.block_diffs_remove(&block_id9).unwrap();
    let res = tx.block_diffs_get(&block_id9).unwrap();
    assert_eq!(res.changes().len(), 0);

    tx.rollback().unwrap();
}

#[test]
fn block_diffs_are_scoped_to_the_queried_branch() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);

    // chain[8] is the last block shared by both branches: chain[9] extends it and `fork` is a sibling of chain[9].
    let fork_point = chain[8].as_leaf();
    let fork = create_block_with_qc(&fork_point);
    fork.insert(&mut tx).unwrap();
    tx.proposal_certificates_save(fork.justify()).unwrap();

    let substate_id = create_random_substate_id();
    let versioned_substate_id = VersionedSubstateId::new(substate_id.clone(), SubstateVersion::ZERO);
    let value = build_substate_value(None);
    tx.block_diffs_insert(fork.id(), &[
        SubstateChange::Down {
            id: versioned_substate_id.clone(),
            shard: fork.shard_group().start(),
        },
        SubstateChange::Up {
            id: substate_id.clone(),
            shard: fork.shard_group().start(),
            substate: Box::new(Substate::new(SubstateVersion::new(1), value)),
        },
    ])
    .unwrap();

    // The changes only exist on the forked-out branch, so they must not be visible from chain[9].
    assert!(
        tx.block_diffs_get_last_change_for_substate(chain[9].id(), &substate_id)
            .optional()
            .unwrap()
            .is_none()
    );
    assert!(
        tx.block_diffs_get_change_for_versioned_substate(chain[9].id(), &versioned_substate_id)
            .optional()
            .unwrap()
            .is_none()
    );

    // ...and they are visible from the branch that contains them.
    let change = tx
        .block_diffs_get_last_change_for_substate(fork.id(), &substate_id)
        .unwrap();
    assert_eq!(change.versioned_substate_id().version(), SubstateVersion::new(1));
    assert!(change.is_up());

    let change = tx
        .block_diffs_get_change_for_versioned_substate(fork.id(), &versioned_substate_id)
        .unwrap();
    assert_eq!(change.versioned_substate_id().version(), SubstateVersion::ZERO);
    assert!(!change.is_up());

    tx.rollback().unwrap();
}

#[test]
fn block_diffs_last_change_prefers_the_down_of_a_version() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);

    let substate_id = create_random_substate_id();
    let versioned_substate_id = VersionedSubstateId::new(substate_id.clone(), SubstateVersion::ZERO);
    let value = build_substate_value(None);
    tx.block_diffs_insert(chain[8].id(), &[SubstateChange::Up {
        id: substate_id.clone(),
        shard: chain[8].shard_group().start(),
        substate: Box::new(Substate::new(SubstateVersion::ZERO, value)),
    }])
    .unwrap();
    tx.block_diffs_insert(chain[9].id(), &[SubstateChange::Down {
        id: versioned_substate_id.clone(),
        shard: chain[9].shard_group().start(),
    }])
    .unwrap();

    let change = tx
        .block_diffs_get_last_change_for_substate(chain[9].id(), &substate_id)
        .unwrap();
    assert!(!change.is_up(), "Expected the DOWN of version 0 but got {change}");

    let change = tx
        .block_diffs_get_change_for_versioned_substate(chain[9].id(), &versioned_substate_id)
        .unwrap();
    assert!(!change.is_up(), "Expected the DOWN of version 0 but got {change}");

    tx.rollback().unwrap();
}

fn up(substate_id: &tari_engine_types::substate::SubstateId, version: u64, block: &Block) -> SubstateChange {
    SubstateChange::Up {
        id: substate_id.clone(),
        shard: block.shard_group().start(),
        substate: Box::new(Substate::new(SubstateVersion::new(version), build_substate_value(None))),
    }
}

fn down(substate_id: &tari_engine_types::substate::SubstateId, version: u64, block: &Block) -> SubstateChange {
    SubstateChange::Down {
        id: VersionedSubstateId::new(substate_id.clone(), SubstateVersion::new(version)),
        shard: block.shard_group().start(),
    }
}

/// What tells two changes apart: the substate version and whether it is UPed or DOWNed.
fn identity(change: &SubstateChange) -> (tari_engine_types::substate::SubstateId, SubstateVersion, bool) {
    let versioned = change.versioned_substate_id();
    (versioned.substate_id().clone(), versioned.version(), change.is_up())
}

fn reopen(db: RocksDbStateStore<String>, tmp: &tempfile::TempDir) -> RocksDbStateStore<String> {
    drop(db);
    RocksDbStateStore::open(tmp.path().join("rocksdb"), DatabaseOptions::default()).unwrap()
}

/// A block's changes survive reopening the store in the order the block made them, and removing them is persisted.
#[test]
fn block_diffs_survive_reopening_the_store() {
    let (db, tmp) = create_rocksdb();
    let chain = create_chain(10);
    let block8 = &chain[8];
    let substate_id = create_random_substate_id();
    // A hot substate changes many times in one block. Its last change is the UP of its highest version.
    let changes = (0..20)
        .flat_map(|v| [up(&substate_id, v, block8), down(&substate_id, v, block8)])
        .chain([up(&substate_id, 20, block8)])
        .collect::<Vec<_>>();
    db.with_write_tx(|tx| {
        commit_chain(tx, &chain);
        tx.block_diffs_insert(block8.id(), &changes)
    })
    .unwrap();

    let db = reopen(db, &tmp);
    {
        let tx = db.create_read_tx().unwrap();
        let diff = tx.block_diffs_get(block8.id()).unwrap();
        assert_eq!(
            diff.changes.iter().map(identity).collect::<Vec<_>>(),
            changes.iter().map(identity).collect::<Vec<_>>(),
            "the block's changes came back in a different order"
        );
        let last = tx
            .block_diffs_get_last_change_for_substate(chain[9].id(), &substate_id)
            .unwrap();
        assert_eq!(identity(&last), identity(&up(&substate_id, 20, block8)));
        let change = tx
            .block_diffs_get_change_for_versioned_substate(
                chain[9].id(),
                &VersionedSubstateId::new(substate_id.clone(), SubstateVersion::new(7)),
            )
            .unwrap();
        assert_eq!(identity(&change), identity(&down(&substate_id, 7, block8)));
    }

    db.with_write_tx(|tx| tx.block_diffs_remove(block8.id())).unwrap();
    let db = reopen(db, &tmp);
    let tx = db.create_read_tx().unwrap();
    assert!(tx.block_diffs_get(block8.id()).unwrap().changes.is_empty());
    assert!(
        tx.block_diffs_get_last_change_for_substate(chain[9].id(), &substate_id)
            .optional()
            .unwrap()
            .is_none()
    );
}

/// A rolled-back transaction's changes are never seen, and a read view sees the changes committed when it opened.
#[test]
fn block_diffs_follow_commits_and_snapshots() {
    let (db, _tmp) = create_rocksdb();
    let chain = create_chain(10);
    let block8 = &chain[8];
    let substate_id = create_random_substate_id();
    db.with_write_tx(|tx| {
        commit_chain(tx, &chain);
        Ok::<_, StorageError>(())
    })
    .unwrap();

    let mut tx = db.create_write_tx().unwrap();
    tx.block_diffs_insert(block8.id(), &[up(&substate_id, 0, block8)])
        .unwrap();
    assert!(
        tx.block_diffs_contains_versioned_substate(
            chain[9].id(),
            &VersionedSubstateId::new(substate_id.clone(), SubstateVersion::ZERO)
        )
        .unwrap(),
        "a write transaction does not see its own changes"
    );
    tx.rollback().unwrap();

    let before = db.create_read_tx().unwrap();
    db.with_write_tx(|tx| tx.block_diffs_insert(block8.id(), &[up(&substate_id, 0, block8)]))
        .unwrap();
    let after = db.create_read_tx().unwrap();
    db.with_write_tx(|tx| tx.block_diffs_remove(block8.id())).unwrap();

    let versioned = VersionedSubstateId::new(substate_id.clone(), SubstateVersion::ZERO);
    assert!(
        !before
            .block_diffs_contains_versioned_substate(chain[9].id(), &versioned)
            .unwrap(),
        "a read view saw changes committed after it was opened, or a rolled-back transaction's changes"
    );
    assert!(
        after
            .block_diffs_contains_versioned_substate(chain[9].id(), &versioned)
            .unwrap(),
        "a read view lost changes removed after it was opened"
    );
}
