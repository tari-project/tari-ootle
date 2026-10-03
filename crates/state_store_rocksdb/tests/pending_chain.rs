//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::BlockId;
use tari_ootle_common_types::{SubstateVersion, VersionedSubstateId};
use tari_ootle_storage::{StateStore, StateStoreReadTransaction, StateStoreWriteTransaction};

pub mod helpers;
use helpers::{commit_chain, create_block_with_qc, create_chain, create_random_substate_id, create_rocksdb};

fn ids<'a, I: IntoIterator<Item = &'a tari_ootle_storage::consensus_models::Block>>(blocks: I) -> Vec<BlockId> {
    blocks.into_iter().map(|b| *b.id()).collect()
}

#[test]
fn it_runs_from_the_leaf_down_to_the_commit_block() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    // Commits chain[..8], so chain[7] is the commit block and chain[8..] are pending.
    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);

    let pending = tx.pending_chain_get(chain[10].id()).unwrap();
    assert_eq!(pending.leaf(), chain[10].id());
    assert_eq!(pending.blocks(), ids(chain[8..].iter().rev()));
    assert!(!pending.contains_pending(chain[7].id()));
    assert!(pending.contains_with_base(chain[7].id()));
    assert!(!pending.contains_with_base(chain[6].id()));
    assert_eq!(pending.commit_position(), Some((chain[7].epoch(), chain[7].height())));

    tx.rollback().unwrap();
}

#[test]
fn it_leaves_out_a_sibling_branch() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);
    let fork = create_block_with_qc(&chain[8].as_leaf());
    fork.insert(&mut tx).unwrap();

    let pending = tx.pending_chain_get(fork.id()).unwrap();
    assert_eq!(pending.blocks(), vec![*fork.id(), *chain[8].id()]);
    assert!(!pending.contains_with_base(chain[9].id()));

    let pending = tx.pending_chain_get(chain[10].id()).unwrap();
    assert!(!pending.contains_with_base(fork.id()));

    tx.rollback().unwrap();
}

#[test]
fn it_is_empty_for_a_committed_leaf() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);

    let pending = tx.pending_chain_get(chain[5].id()).unwrap();
    assert!(pending.is_empty());
    // A committed leaf still exists, so reads at it answer from the committed state.
    let versioned = VersionedSubstateId::new(create_random_substate_id(), SubstateVersion::ZERO);
    assert!(
        !tx.block_diffs_contains_versioned_substate_in_chain(&pending, &versioned)
            .unwrap()
    );

    tx.rollback().unwrap();
}

#[test]
fn it_fails_a_read_at_a_leaf_that_does_not_exist() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);
    let missing = create_block_with_qc(&chain[10].as_leaf());

    let pending = tx.pending_chain_get(missing.id()).unwrap();
    let versioned = VersionedSubstateId::new(create_random_substate_id(), SubstateVersion::ZERO);
    assert!(
        tx.block_diffs_contains_versioned_substate_in_chain(&pending, &versioned)
            .is_err()
    );

    tx.rollback().unwrap();
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "is stale")]
fn a_chain_read_before_its_leaf_is_inserted_is_stale() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);
    let next = create_block_with_qc(&chain[10].as_leaf());

    let stale = tx.pending_chain_get(next.id()).unwrap();
    next.insert(&mut tx).unwrap();

    let versioned = VersionedSubstateId::new(create_random_substate_id(), SubstateVersion::ZERO);
    tx.block_diffs_contains_versioned_substate_in_chain(&stale, &versioned)
        .unwrap();
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "is stale")]
fn a_chain_read_before_a_commit_is_stale() {
    let (db, _tmp) = create_rocksdb();
    let mut tx = db.create_write_tx().unwrap();

    let chain = create_chain(10);
    commit_chain(&mut tx, &chain);

    let stale = tx.pending_chain_get(chain[10].id()).unwrap();
    tx.blocks_set_qcs(chain[8].id(), Some(&tari_consensus_types::PcId::zero()), None)
        .unwrap();

    tx.substate_locks_get_latest_for_substate_in_chain(&stale, &create_random_substate_id())
        .unwrap_err();
}
