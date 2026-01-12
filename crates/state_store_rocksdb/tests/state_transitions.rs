//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod helpers;

use std::collections::{HashMap, HashSet};

use helpers::{create_rocksdb, create_substate_update_batch, gen_substates};
use tari_ootle_common_types::{Epoch, Network};
use tari_ootle_storage::{
    consensus_models::{Block, SubstateValueFilterFlags},
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};
use tari_state_tree::Version;

use crate::helpers::num_preshards;

#[test]
fn rocksdb() {
    let (db, _tmp) = create_rocksdb();

    let num_transitions = 100; // Makes double
    const EPOCH: Epoch = Epoch::zero();
    let mut tx = db.create_write_tx().unwrap();

    let zero_block = Block::zero_block(Network::LocalNet, num_preshards());
    zero_block.insert(&mut tx).unwrap();

    let mut shards = HashMap::new();
    let substates = gen_substates(EPOCH, 1, 0..num_transitions, 0).collect::<Vec<_>>();
    shards.insert(
        1 as Version,
        (
            substates.len(),
            substates.iter().map(|s| s.shard()).collect::<HashSet<_>>(),
        ),
    );
    let batch = create_substate_update_batch(Epoch::zero(), &substates);
    tx.substates_commit_batch(batch).unwrap();

    // Add a couple for a different shard
    let substates = gen_substates(EPOCH, 2, num_transitions..num_transitions + 2, 0).collect::<Vec<_>>();
    shards.insert(
        2,
        (
            substates.len(),
            substates.iter().map(|s| s.shard()).collect::<HashSet<_>>(),
        ),
    );
    let batch = create_substate_update_batch(Epoch::zero(), &substates);
    tx.substates_commit_batch(batch).unwrap();

    let substates = gen_substates(EPOCH, 3, 0..num_transitions, 1).collect::<Vec<_>>();
    shards.insert(
        3,
        (
            substates.len(),
            substates.iter().map(|s| s.shard()).collect::<HashSet<_>>(),
        ),
    );
    let batch = create_substate_update_batch(Epoch::zero(), &substates);
    tx.substates_commit_batch(batch).unwrap();

    for (state_version, (num_substates, shards)) in &shards {
        for shard in shards {
            let transitions = tx
                .state_transitions_get_starting_at(*shard, *state_version, SubstateValueFilterFlags::all())
                .unwrap();
            assert_eq!(transitions.epoch, EPOCH);
            assert_eq!(transitions.state_version, *state_version);
            assert_eq!(transitions.shard, *shard);
            assert_eq!(transitions.updates.len(), *num_substates);
        }
    }
}
