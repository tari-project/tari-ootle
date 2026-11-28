//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod helpers;
use std::collections::HashSet;

use helpers::{assert_eq_debug, build_substate_record, create_rocksdb, create_substate_update_batch};
use indexmap::IndexMap;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{
    Epoch,
    SubstateLockType,
    SubstateVersion,
    VersionedSubstateId,
    VersionedSubstateIdRef,
    shard::Shard,
};
use tari_ootle_storage::{
    EpochCleanupStep,
    ShardScopedTreeStoreWriter,
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StateTreeRetention,
    consensus_models::{
        Block,
        SubstateDestroyed,
        SubstateLock,
        SubstateTransition,
        SubstateUpdateBatch,
        SubstateValueFilterFlags,
    },
};
use tari_ootle_transaction::{Network, TransactionId};
use tari_state_store_rocksdb::DatabaseOptions;
use tari_state_tree::{StateTree, SubstateTreeChange, key_mapper::SpreadPrefixKeyMapper};
use tari_template_lib::types::{ComponentAddress, ObjectKey};

use crate::helpers::{create_rocksdb_with_opts, gen_substates, num_preshards};

fn substate_id(seed: u8) -> SubstateId {
    let address = ComponentAddress::from_array([seed; ObjectKey::LENGTH]);
    SubstateId::Component(address)
}

#[test]
fn basic_operations() {
    let (db, _tmp) = create_rocksdb();

    let mut tx = db.create_write_tx().unwrap();

    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();

    // substate 1
    let substate1_id = substate_id(1);
    let substate1 = build_substate_record(&substate1_id, SubstateVersion::ZERO, 1);
    let substate1_address = substate1.to_substate_address();
    // substate 1 (version 1)
    let substate1b = build_substate_record(&substate1_id, SubstateVersion::new(1), 1);
    let substate1b_address = substate1b.to_substate_address();
    // substate 2
    let substate2_id = substate_id(2);
    let substate2 = build_substate_record(&substate2_id, SubstateVersion::ZERO, 1);
    let substate2_address = substate2.to_substate_address();

    let batch = create_substate_update_batch(Epoch::zero(), [&substate1, &substate1b, &substate2]);
    tx.substates_commit_batch(batch).unwrap();

    // check that we can get all the newly inserted substates
    let res = tx.substates_get(&substate1_address).unwrap();
    assert_eq_debug(&res.substate_value, &substate1.substate_value);

    let res = tx.substates_get(&substate1b_address).unwrap();
    assert_eq_debug(&res, &substate1b);

    let res = tx.substates_get(&substate2_address).unwrap();
    assert_eq_debug(&res, &substate2);

    // substates_get_any fetches all substates
    let req = [
        VersionedSubstateIdRef::new(&substate1_id, SubstateVersion::ZERO),
        VersionedSubstateIdRef::new(&substate2_id, SubstateVersion::ZERO),
    ];
    let res = tx.substates_get_any(&req).unwrap();
    assert_eq!(res.len(), 2);

    // substates_get_any fetches the last version of a substate
    let mut req = HashSet::new();
    req.insert(VersionedSubstateIdRef::new(&substate1_id, SubstateVersion::ZERO));
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
    assert!(
        res.iter()
            .any(|s| s.substate_id == substate1_id && s.version == SubstateVersion::new(1))
    );
    assert!(
        res.iter()
            .any(|s| s.substate_id == substate2_id && s.version == SubstateVersion::ZERO)
    );

    // substates_get_max_version_for_substate
    let res = tx.substates_get_max_version_for_substate(&substate1_id).unwrap();
    assert_eq!(res, (SubstateVersion::new(1), true));
    let res = tx.substates_get_max_version_for_substate(&substate2_id).unwrap();
    assert_eq!(res, (SubstateVersion::ZERO, true));

    // substates_any_exist (all exist)
    let substate_ids = [
        VersionedSubstateId::new(substate1_id.clone(), SubstateVersion::ZERO),
        VersionedSubstateId::new(substate2_id.clone(), SubstateVersion::ZERO),
    ];
    let res = tx
        .substates_any_exist(substate_ids.iter().map(|id| id.as_versioned_ref()))
        .unwrap();
    assert!(res);

    // substates_any_exist (some do not exist)
    let substate_ids = [
        VersionedSubstateId::new(substate1_id.clone(), SubstateVersion::new(100)), // version should not exist
        VersionedSubstateId::new(substate2_id.clone(), SubstateVersion::ZERO),
    ];
    let res = tx
        .substates_any_exist(substate_ids.iter().map(|id| id.as_versioned_ref()))
        .unwrap();
    assert!(res);

    // substates_any_exist (none exist)
    let substate_ids = [
        VersionedSubstateId::new(substate1_id, SubstateVersion::new(100)), // version should not exist
        VersionedSubstateId::new(substate2_id, SubstateVersion::new(100)), // version should not exist
    ];
    let res = tx
        .substates_any_exist(substate_ids.iter().map(|id| id.as_versioned_ref()))
        .unwrap();
    assert!(!res);

    // substates_down
    let res = tx.substates_get(&substate2_address).unwrap();
    assert!(res.destroyed.is_none());

    let versioned_substate_id = VersionedSubstateId::new(substate2.substate_id, substate2.version);
    let shard = versioned_substate_id.to_shard(num_preshards());
    let epoch = Epoch::zero();

    let mut batch = SubstateUpdateBatch::new(helpers::NETWORK, epoch);
    batch
        .with_transition(shard, 2)
        .push(tari_ootle_storage::consensus_models::SubstateTransition::Down {
            id: versioned_substate_id.clone(),
        });
    tx.substates_commit_batch(batch).unwrap();
    let res = tx.substates_get(&substate2_address).unwrap();
    assert!(res.destroyed.is_some());

    tx.rollback().unwrap();
}

#[test]
fn substate_head_iter() {
    type SpreadStateTree<'a, D> = StateTree<'a, D, SpreadPrefixKeyMapper>;
    const SHARD: Shard = Shard::first();

    let (db, _tmp) = create_rocksdb_with_opts(
        DatabaseOptions::default()
            .with_state_history_length(0)
            .with_epoch_history_length(0),
    );

    let mut tx = db.create_write_tx().unwrap();

    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();

    let substates = gen_substates(Epoch::zero(), 1, SHARD, 100, SubstateVersion::ZERO).collect::<Vec<_>>();

    let batch = create_substate_update_batch(Epoch::zero(), substates.iter());
    let changes = substates.iter().map(|s| SubstateTreeChange::Up {
        id: VersionedSubstateId::new(s.substate_id.clone(), s.version),
        value_hash: *s.state_hash(),
    });

    {
        // Put the tree changes and commit the substates
        let mut store = ShardScopedTreeStoreWriter::new(&mut tx, SHARD);
        let _mr = SpreadStateTree::new(&mut store)
            .batch_put_substate_changes(None, 1, changes)
            .unwrap();
    }
    tx.state_tree_shard_versions_set(SHARD, 1).unwrap();
    tx.substates_commit_batch(batch).unwrap();
    let iter = tx
        .substate_tree_iter(SHARD, 1, SubstateValueFilterFlags::all_substates())
        .unwrap();

    let count = iter.count();
    assert_eq!(count, 100);

    let substates_to_update = substates.iter().take(50);

    let downs = substates_to_update.clone().map(|s| SubstateTreeChange::Down {
        id: VersionedSubstateId::new(s.substate_id.clone(), s.version),
    });
    let ups = substates_to_update.clone().map(|s| SubstateTreeChange::Up {
        id: VersionedSubstateId::new(s.substate_id.clone(), s.version.next()),
        value_hash: *s.state_hash(),
    });
    {
        let mut store = ShardScopedTreeStoreWriter::new(&mut tx, SHARD);
        let _mr = SpreadStateTree::new(&mut store)
            .batch_put_substate_changes(Some(1), 2, downs.chain(ups))
            .unwrap();
    }
    tx.state_tree_shard_versions_set(SHARD, 2).unwrap();

    let mut batch = SubstateUpdateBatch::new(helpers::NETWORK, Epoch::zero());

    for s in substates_to_update {
        // Mark the old substate as destroyed
        batch.with_transition(SHARD, 2).push(SubstateTransition::Down {
            id: s.to_versioned_substate_id(),
        });
        // Create a new version of the substate
        batch.with_transition(SHARD, 2).push(SubstateTransition::Up {
            id: s.substate_id.clone(),
            version: s.version.next(),
            substate_or_hash: s.clone().into_substate_value_or_hash(),
        });
    }

    tx.substates_commit_batch(batch).unwrap();

    // TODO: substate_head_iter only works correctly after pruning downed values.
    // This is because we iterate the nodes as a "flat" list, a correct iterator would need to incorporate JMT logic.
    let retention = StateTreeRetention::read(&*tx, tari_ootle_common_types::NumPreshards::current(), 0).unwrap();
    tx.state_tree_nodes_clear_stale(&retention, usize::MAX).unwrap();

    let iter = tx
        .substate_tree_iter(SHARD, 0, SubstateValueFilterFlags::all_substates())
        .unwrap();

    let mut count = 0;
    let mut seen = HashSet::new();
    for (i, r) in iter.enumerate() {
        let (state_version, id, s) = r.unwrap();
        if !seen.insert(VersionedSubstateId::new(id.clone(), s.version())) {
            panic!("{i} {state_version} Duplicate substate id {}v{}", id, s.version());
        }

        count += 1;
    }
    assert_eq!(count, 100);

    tx.rollback().unwrap();
}

/// A substate's head is its highest version, and DOWN once that version is put down, whatever order a state version
/// lists the substate's transitions in.
#[test]
fn substate_head_is_the_highest_version_in_any_transition_order() {
    fn up(id: &SubstateId, version: u64) -> SubstateTransition {
        let version = SubstateVersion::new(version);
        SubstateTransition::Up {
            id: id.clone(),
            version,
            substate_or_hash: build_substate_record(id, version, 0).into_substate_value_or_hash(),
        }
    }

    fn down(id: &SubstateId, version: u64) -> SubstateTransition {
        SubstateTransition::Down {
            id: VersionedSubstateId::new(id.clone(), SubstateVersion::new(version)),
        }
    }

    fn batch(id: &SubstateId, state_version: u64, transitions: Vec<SubstateTransition>) -> SubstateUpdateBatch {
        let shard = VersionedSubstateIdRef::new(id, SubstateVersion::ZERO).to_shard(num_preshards());
        let mut batch = SubstateUpdateBatch::new(helpers::NETWORK, Epoch::zero());
        batch.with_transition(shard, state_version).extend(transitions);
        batch
    }

    let (db, _tmp) = create_rocksdb();
    let id = substate_id(1);

    // One state version takes the substate from version 0 to version 2. Each order lists the same transitions, with
    // every version put up before it is put down.
    let orders = [
        vec![down(&id, 0), up(&id, 1), down(&id, 1), up(&id, 2)],
        vec![up(&id, 1), down(&id, 0), up(&id, 2), down(&id, 1)],
        vec![up(&id, 1), up(&id, 2), down(&id, 1), down(&id, 0)],
    ];
    for transitions in orders {
        let mut tx = db.create_write_tx().unwrap();
        tx.substates_commit_batch(batch(&id, 1, vec![up(&id, 0)])).unwrap();
        tx.substates_commit_batch(batch(&id, 2, transitions)).unwrap();

        assert_eq!(
            tx.substates_get_max_version_for_substate(&id).unwrap(),
            (SubstateVersion::new(2), true)
        );
        tx.rollback().unwrap();
    }

    // A version put up and down by one state version is left down
    let mut tx = db.create_write_tx().unwrap();
    tx.substates_commit_batch(batch(&id, 1, vec![up(&id, 0), down(&id, 0)]))
        .unwrap();
    assert_eq!(
        tx.substates_get_max_version_for_substate(&id).unwrap(),
        (SubstateVersion::ZERO, false)
    );
    tx.rollback().unwrap();
}

/// Pruning reaches every epoch at or before the prune epoch, so values downed in an epoch whose GC never ran, or was
/// interrupted, are cleared by a later run.
#[test]
fn downed_values_of_earlier_epochs_are_pruned_in_batches() {
    let (db, _tmp) = create_rocksdb_with_opts(
        DatabaseOptions::default()
            .with_epoch_history_length(1)
            .with_pinned_checkpoint_epochs(1),
    );
    let ups = [10, 11].map(|seed| build_substate_record(&substate_id(seed), SubstateVersion::ZERO, 1));
    db.with_write_tx(|tx| tx.substates_commit_batch(create_substate_update_batch(Epoch(1), &ups)))
        .unwrap();
    for (epoch, up) in [Epoch(1), Epoch(2)].into_iter().zip(&ups) {
        let mut down = up.clone();
        down.set_destroyed(SubstateDestroyed {
            at_epoch: epoch,
            at_state_version: 1 + epoch.as_u64(),
        });
        db.with_write_tx(|tx| tx.substates_commit_batch(create_substate_update_batch(epoch, [&down])))
            .unwrap();
    }

    let prune = || {
        db.with_write_tx(|tx| tx.epoch_cleanup_step(Epoch(3), EpochCleanupStep::DownedSubstateValues, 1))
            .unwrap()
    };
    assert_eq!(prune(), 1);
    assert_eq!(prune(), 1);
    assert_eq!(prune(), 0);

    db.with_read_tx(|tx| {
        for up in &ups {
            let rec = tx.substates_get(&up.to_substate_address())?;
            assert!(rec.is_destroyed());
            assert!(
                rec.substate_value().is_none(),
                "value of {} was not pruned",
                rec.substate_id
            );
        }
        Ok::<_, tari_ootle_storage::StorageError>(())
    })
    .unwrap();
}

/// A transaction that locked a version and has not yet released it pledges and executes that version's value, so the
/// value is kept past the retention window until the lock is released.
#[test]
fn a_downed_value_is_kept_while_a_lock_names_its_version() {
    let (db, _tmp) = create_rocksdb_with_opts(
        DatabaseOptions::default()
            .with_epoch_history_length(1)
            .with_pinned_checkpoint_epochs(1),
    );
    let chain = helpers::create_chain(3);
    let [locked, free] = [13, 14].map(|seed| build_substate_record(&substate_id(seed), SubstateVersion::ZERO, 1));
    let downs = [&locked, &free].map(|up| {
        let mut down = up.clone();
        down.set_destroyed(SubstateDestroyed {
            at_epoch: Epoch(2),
            at_state_version: 2,
        });
        down
    });
    let transaction_id = TransactionId::new([1; 32]);
    db.with_write_tx(|tx| {
        helpers::commit_chain(tx, &chain);
        tx.substates_commit_batch(create_substate_update_batch(Epoch(1), [&locked, &free]))?;
        tx.substates_commit_batch(create_substate_update_batch(Epoch(2), &downs))?;
        tx.substate_locks_insert_all(
            &chain[2].as_leaf(),
            &IndexMap::from([(locked.substate_id.clone(), vec![SubstateLock::new(
                transaction_id,
                SubstateVersion::ZERO,
                SubstateLockType::Write,
                false,
            )])]),
        )
    })
    .unwrap();

    let prune = |epoch| {
        db.with_write_tx(|tx| tx.epoch_cleanup_step(epoch, EpochCleanupStep::DownedSubstateValues, usize::MAX))
            .unwrap()
    };
    let has_value = |rec: &tari_ootle_storage::consensus_models::SubstateRecord| {
        db.with_read_tx(|tx| {
            let rec = tx.substates_get(&rec.to_substate_address())?;
            Ok::<_, tari_ootle_storage::StorageError>(rec.substate_value().is_some())
        })
        .unwrap()
    };

    assert_eq!(prune(Epoch(3)), 2);
    assert!(has_value(&locked), "the value of a locked version was pruned");
    assert!(!has_value(&free));
    // A cleanup of the same epoch does not examine the kept value again
    assert_eq!(prune(Epoch(3)), 0);
    assert_eq!(prune(Epoch(4)), 1);
    assert!(has_value(&locked), "the value of a locked version was pruned");

    db.with_write_tx(|tx| tx.substate_locks_remove_many_for_transactions([&transaction_id]))
        .unwrap();
    assert_eq!(prune(Epoch(5)), 1);
    assert!(!has_value(&locked), "the value outlived the lock on its version");
    assert_eq!(prune(Epoch(6)), 0);
}

/// A pinned tree is served with the values of the substates it holds, so a value is kept for as many epochs as
/// checkpoints are pinned when that exceeds the epoch history.
#[test]
fn downed_values_are_kept_for_the_pinned_checkpoint_epochs() {
    let (db, _tmp) = create_rocksdb_with_opts(
        DatabaseOptions::default()
            .with_epoch_history_length(1)
            .with_pinned_checkpoint_epochs(2),
    );
    let up = build_substate_record(&substate_id(12), SubstateVersion::ZERO, 1);
    db.with_write_tx(|tx| tx.substates_commit_batch(create_substate_update_batch(Epoch(1), [&up])))
        .unwrap();
    let mut down = up.clone();
    down.set_destroyed(SubstateDestroyed {
        at_epoch: Epoch(2),
        at_state_version: 3,
    });
    db.with_write_tx(|tx| tx.substates_commit_batch(create_substate_update_batch(Epoch(2), [&down])))
        .unwrap();

    let prune = |epoch| {
        db.with_write_tx(|tx| tx.epoch_cleanup_step(epoch, EpochCleanupStep::DownedSubstateValues, usize::MAX))
            .unwrap()
    };
    let has_value = || {
        db.with_read_tx(|tx| {
            let rec = tx.substates_get(&up.to_substate_address())?;
            Ok::<_, tari_ootle_storage::StorageError>(rec.substate_value().is_some())
        })
        .unwrap()
    };
    assert_eq!(prune(Epoch(3)), 0);
    assert!(has_value());
    assert_eq!(prune(Epoch(4)), 1);
    assert!(!has_value());
}
