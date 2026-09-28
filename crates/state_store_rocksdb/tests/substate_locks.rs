//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod helpers;

use std::collections::HashMap;

use indexmap::IndexMap;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{SubstateLockType, SubstateVersion, optional::Optional};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    consensus_models::SubstateLock,
};
use tari_ootle_transaction::TransactionId;

use crate::helpers::{
    commit_chain,
    create_block_with_qc,
    create_chain,
    create_random_substate_id,
    create_rocksdb,
    substate_id_tx_seed,
    transaction_id_from_seed,
};

#[test]
fn rocksdb() {
    // env_logger::builder().filter_level(log::LevelFilter::Debug).init();
    let (db, _tmp) = create_rocksdb();
    run_test(db);
}

fn run_test(db: impl StateStore) {
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);
    let b7 = chain[7].as_leaf();
    let b8 = chain[8].as_leaf();
    let b9 = chain[9].as_leaf();

    log::debug!("b7: {}, b8: {}, b9: {}", b7, b8, b9);

    let s1 = create_random_substate_id();
    let s = tx
        .substate_locks_get_latest_for_substate(&chain[0].as_leaf(), &s1)
        .optional()
        .unwrap();
    assert!(s.is_none());

    let tx_1 = transaction_id_from_seed(1);
    let tx_1_locks = gen_locks(tx_1, 5).collect::<IndexMap<_, _>>();
    let tx_2 = transaction_id_from_seed(2);
    let tx_2_locks = gen_locks(tx_2, 5).collect::<IndexMap<_, _>>();
    let tx_3 = transaction_id_from_seed(3);
    let tx_3_locks = gen_locks(tx_3, 5).collect::<IndexMap<_, _>>();
    let tx_4 = transaction_id_from_seed(4);
    let tx_4_locks = gen_locks(tx_4, 5).collect::<IndexMap<_, _>>();

    let mut locks_for_b8 = IndexMap::new();
    for (substate_id, lock) in tx_1_locks.iter().chain(tx_2_locks.iter()) {
        let v = locks_for_b8.entry(substate_id.clone()).or_insert_with(Vec::new);
        v.push(*lock);
    }

    tx.substate_locks_insert_all(&b8, &locks_for_b8).unwrap();
    let mut locks_for_b9 = IndexMap::new();
    for (substate_id, lock) in tx_3_locks.iter().chain(tx_4_locks.iter()) {
        let v = locks_for_b9.entry(substate_id.clone()).or_insert_with(Vec::new);
        v.push(*lock);
    }
    tx.substate_locks_insert_all(&b9, &locks_for_b9).unwrap();

    let mut all_locks = IndexMap::new();
    for (substate_id, lock) in tx_1_locks
        .iter()
        .chain(tx_2_locks.iter())
        .chain(tx_3_locks.iter())
        .chain(tx_4_locks.iter())
    {
        let v = all_locks.entry(substate_id.clone()).or_insert_with(Vec::new);
        v.push(*lock);
    }

    let mut tx_id_counts = HashMap::new();
    for locks in all_locks.values() {
        for lock in locks {
            let count = tx_id_counts.entry(lock.transaction_id()).or_insert(0usize);
            *count += 1;
        }
    }

    for (id, locks) in &all_locks {
        let s = tx.substate_locks_get_latest_for_substate(&b9, id).unwrap();
        let l = locks.last().unwrap();
        assert_eq!(s.lock_type(), l.lock_type());
        assert_eq!(s.version(), l.version());

        let locked_by_tx = tx
            .substate_locks_get_locked_substates_for_transaction(l.transaction_id())
            .unwrap();
        assert_eq!(locked_by_tx.len(), *tx_id_counts.get(l.transaction_id()).unwrap());
    }
    for id in locks_for_b9.keys() {
        let s = tx.substate_locks_get_latest_for_substate(&b8, id).optional().unwrap();
        assert!(s.is_none());
    }

    tx.substate_locks_remove_many_for_transactions(Some(&tx_1)).unwrap();
    let locked_by_tx = tx.substate_locks_get_locked_substates_for_transaction(&tx_1).unwrap();
    assert_eq!(locked_by_tx.len(), 0);

    tx.substate_locks_remove_any_by_block_id(b9.block_id()).unwrap();
    let locked_by_tx = tx.substate_locks_get_locked_substates_for_transaction(&tx_3).unwrap();
    assert_eq!(locked_by_tx.len(), 0);
    let locked_by_tx = tx.substate_locks_get_locked_substates_for_transaction(&tx_4).unwrap();
    assert_eq!(locked_by_tx.len(), 0);

    tx.rollback().unwrap();
}

fn gen_locks(transaction_id: TransactionId, num: usize) -> impl Iterator<Item = (SubstateId, SubstateLock)> {
    (0..num as u64).map(move |i| {
        let id = substate_id_tx_seed(transaction_id, i as u32);
        let lock = SubstateLock::new(transaction_id, SubstateVersion::new(i), SubstateLockType::Write, false);
        (id, lock)
    })
}

/// A block grants several locks on one substate in command order, and the chain's answer is the last of them.
///
/// Key order follows the transaction id, which has nothing to do with the order the block granted the locks, so a
/// lookup that resolves the ordering from the key shape answers differently depending on which path it took - and which
/// path it takes depends on which other branches the node happens to have written.
#[test]
fn the_latest_lock_does_not_depend_on_other_branches() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);
    let b8 = chain[8].as_leaf();
    let b9 = chain[9].as_leaf();

    let substate_id = create_random_substate_id();
    // Order the ids so that the lock granted first is also the one that sorts first, which is what makes a key-ordered
    // answer differ from the granted one.
    let (granted_first, granted_last) = ordered_transaction_ids();

    tx.substate_locks_insert_all(&b8, &two_locks(&substate_id, granted_first, granted_last))
        .unwrap();

    let lock = tx.substate_locks_get_latest_for_substate(&b9, &substate_id).unwrap();
    assert_eq!(lock.transaction_id(), &granted_last);
    assert_eq!(lock.lock_type(), SubstateLockType::Output);

    // A sibling of b9 locks the same substate. It is not on b9's chain, so b9's answer must not move.
    let fork = create_block_with_qc(&b8);
    tx.proposal_certificates_save(fork.justify()).unwrap();
    fork.insert(&mut tx).unwrap();
    let fork_locks = IndexMap::from([(substate_id.clone(), vec![SubstateLock::new(
        transaction_id_from_seed(9),
        SubstateVersion::new(1),
        SubstateLockType::Output,
        true,
    )])]);
    tx.substate_locks_insert_all(&fork.as_leaf(), &fork_locks).unwrap();

    let lock = tx.substate_locks_get_latest_for_substate(&b9, &substate_id).unwrap();
    assert_eq!(
        lock.transaction_id(),
        &granted_last,
        "writing a sibling branch changed the answer for b9"
    );
    assert_eq!(lock.lock_type(), SubstateLockType::Output);

    tx.rollback().unwrap();
}

/// A lock a committed block granted is ordered the same way as a pending one, so the answer is still the last granted.
#[test]
fn the_latest_committed_lock_is_the_one_granted_last() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);
    // commit_chain commits every block below the last three, so this block is beneath the commit block and no longer in
    // the pending chain.
    let committed = chain[6].as_leaf();
    let b9 = chain[9].as_leaf();

    let substate_id = create_random_substate_id();
    let (granted_first, granted_last) = ordered_transaction_ids();

    tx.substate_locks_insert_all(&committed, &two_locks(&substate_id, granted_first, granted_last))
        .unwrap();

    let lock = tx.substate_locks_get_latest_for_substate(&b9, &substate_id).unwrap();
    assert_eq!(lock.transaction_id(), &granted_last);
    assert_eq!(lock.lock_type(), SubstateLockType::Output);

    tx.rollback().unwrap();
}

/// A branch the leaf does not extend holds no locks as far as that leaf is concerned.
#[test]
fn a_lock_from_another_branch_is_not_found() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);
    let b8 = chain[8].as_leaf();
    let b9 = chain[9].as_leaf();

    let fork = create_block_with_qc(&b8);
    tx.proposal_certificates_save(fork.justify()).unwrap();
    fork.insert(&mut tx).unwrap();

    let substate_id = create_random_substate_id();
    let locks = IndexMap::from([(substate_id.clone(), vec![SubstateLock::new(
        transaction_id_from_seed(1),
        SubstateVersion::new(0),
        SubstateLockType::Write,
        false,
    )])]);
    tx.substate_locks_insert_all(&fork.as_leaf(), &locks).unwrap();

    let lock = tx
        .substate_locks_get_latest_for_substate(&b9, &substate_id)
        .optional()
        .unwrap();
    assert!(lock.is_none());

    // The branch that granted it sees it
    let lock = tx
        .substate_locks_get_latest_for_substate(&fork.as_leaf(), &substate_id)
        .unwrap();
    assert_eq!(lock.transaction_id(), &transaction_id_from_seed(1));

    tx.rollback().unwrap();
}

/// Releasing one transaction's lock must leave the other locks on the substate reachable.
#[test]
fn releasing_one_transactions_lock_leaves_the_rest() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);
    let b8 = chain[8].as_leaf();
    let b9 = chain[9].as_leaf();

    let substate_id = create_random_substate_id();
    let (granted_first, granted_last) = ordered_transaction_ids();

    tx.substate_locks_insert_all(&b8, &two_locks(&substate_id, granted_first, granted_last))
        .unwrap();

    tx.substate_locks_remove_many_for_transactions(Some(&granted_last))
        .unwrap();

    let lock = tx.substate_locks_get_latest_for_substate(&b9, &substate_id).unwrap();
    assert_eq!(lock.transaction_id(), &granted_first);
    assert_eq!(lock.lock_type(), SubstateLockType::Read);

    tx.substate_locks_remove_many_for_transactions(Some(&granted_first))
        .unwrap();

    let lock = tx
        .substate_locks_get_latest_for_substate(&b9, &substate_id)
        .optional()
        .unwrap();
    assert!(lock.is_none());

    tx.rollback().unwrap();
}

/// Removing a block releases every lock it granted, including the ones the chain-order index holds.
#[test]
fn removing_a_block_releases_every_lock_it_granted() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);
    let b8 = chain[8].as_leaf();
    let b9 = chain[9].as_leaf();

    let substate_id = create_random_substate_id();
    let (granted_first, granted_last) = ordered_transaction_ids();

    tx.substate_locks_insert_all(&b8, &two_locks(&substate_id, granted_first, granted_last))
        .unwrap();
    tx.substate_locks_remove_any_by_block_id(b8.block_id()).unwrap();

    let lock = tx
        .substate_locks_get_latest_for_substate(&b9, &substate_id)
        .optional()
        .unwrap();
    assert!(lock.is_none());

    tx.rollback().unwrap();
}

/// Two transaction ids, the lower first, so that granting in that order puts the latest lock last in key order too.
fn ordered_transaction_ids() -> (TransactionId, TransactionId) {
    let mut ids = [transaction_id_from_seed(1), transaction_id_from_seed(2)];
    ids.sort();
    (ids[0], ids[1])
}

/// A READ lock followed by an OUTPUT lock on one substate, as local-only rules allow within one block.
fn two_locks(
    substate_id: &SubstateId,
    granted_first: TransactionId,
    granted_last: TransactionId,
) -> IndexMap<SubstateId, Vec<SubstateLock>> {
    IndexMap::from([(substate_id.clone(), vec![
        SubstateLock::new(granted_first, SubstateVersion::new(0), SubstateLockType::Read, true),
        SubstateLock::new(granted_last, SubstateVersion::new(1), SubstateLockType::Output, true),
    ])])
}

/// A branch block below the commit height holds no locks for the chain that committed that height.
///
/// Height alone does not place a lock on the committed chain: a branch block can sit below the commit block. It is
/// still in the pending chain, which is what tells the two apart.
#[test]
fn a_lock_from_a_branch_below_the_commit_height_is_not_found() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);
    let b9 = chain[9].as_leaf();

    // A sibling of chain[6], so it sits below the commit block that commit_chain leaves at chain[7]
    let branch = create_block_with_qc(&chain[5].as_leaf());
    tx.proposal_certificates_save(branch.justify()).unwrap();
    branch.insert(&mut tx).unwrap();
    assert!(branch.height() < chain[7].height());

    let substate_id = create_random_substate_id();
    let locks = IndexMap::from([(substate_id.clone(), vec![SubstateLock::new(
        transaction_id_from_seed(1),
        SubstateVersion::new(0),
        SubstateLockType::Write,
        false,
    )])]);
    tx.substate_locks_insert_all(&branch.as_leaf(), &locks).unwrap();

    let lock = tx
        .substate_locks_get_latest_for_substate(&b9, &substate_id)
        .optional()
        .unwrap();
    assert!(lock.is_none());

    tx.rollback().unwrap();
}
