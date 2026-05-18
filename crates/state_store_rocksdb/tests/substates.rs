//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod helpers;
use std::collections::HashSet;

use helpers::{assert_eq_debug, build_substate_record, create_rocksdb, create_substate_update_batch};
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{Epoch, VersionedSubstateId, VersionedSubstateIdRef, shard::Shard};
use tari_ootle_storage::{
    ShardScopedTreeStoreWriter,
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    consensus_models::{Block, SubstateTransition, SubstateUpdateBatch, SubstateValueFilterFlags},
};
use tari_ootle_transaction::Network;
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
    let substate1 = build_substate_record(&substate1_id, 0, 1);
    let substate1_address = substate1.to_substate_address();
    // substate 1 (version 1)
    let substate1b = build_substate_record(&substate1_id, 1, 1);
    let substate1b_address = substate1b.to_substate_address();
    // substate 2
    let substate2_id = substate_id(2);
    let substate2 = build_substate_record(&substate2_id, 0, 1);
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
        .substates_any_exist(substate_ids.iter().map(|id| id.as_versioned_ref()))
        .unwrap();
    assert!(res);

    // substates_any_exist (some do not exist)
    let substate_ids = [
        VersionedSubstateId::new(substate1_id.clone(), 100), // version should not exist
        VersionedSubstateId::new(substate2_id.clone(), 0),
    ];
    let res = tx
        .substates_any_exist(substate_ids.iter().map(|id| id.as_versioned_ref()))
        .unwrap();
    assert!(res);

    // substates_any_exist (none exist)
    let substate_ids = [
        VersionedSubstateId::new(substate1_id, 100), // version should not exist
        VersionedSubstateId::new(substate2_id, 100), // version should not exist
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

    let mut batch = SubstateUpdateBatch::new(epoch);
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

    let substates = gen_substates(Epoch::zero(), 1, SHARD, 100, 0).collect::<Vec<_>>();

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
        id: VersionedSubstateId::new(s.substate_id.clone(), s.version + 1),
        value_hash: *s.state_hash(),
    });
    {
        let mut store = ShardScopedTreeStoreWriter::new(&mut tx, SHARD);
        let _mr = SpreadStateTree::new(&mut store)
            .batch_put_substate_changes(Some(1), 2, downs.chain(ups))
            .unwrap();
    }
    tx.state_tree_shard_versions_set(SHARD, 2).unwrap();

    let mut batch = SubstateUpdateBatch::new(Epoch::zero());

    for s in substates_to_update {
        // Mark the old substate as destroyed
        batch.with_transition(SHARD, 2).push(SubstateTransition::Down {
            id: s.to_versioned_substate_id(),
        });
        // Create a new version of the substate
        batch.with_transition(SHARD, 2).push(SubstateTransition::Up {
            id: s.substate_id.clone(),
            version: s.version + 1,
            substate_or_hash: s.clone().into_substate_value_or_hash(),
        });
    }

    tx.substates_commit_batch(batch).unwrap();

    // TODO: substate_head_iter only works correctly after pruning downed values.
    // This is because we iterate the nodes as a "flat" list, a correct iterator would need to incorporate JMT logic.
    tx.state_tree_nodes_clear_stale(tari_ootle_common_types::NumPreshards::current())
        .unwrap();

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
