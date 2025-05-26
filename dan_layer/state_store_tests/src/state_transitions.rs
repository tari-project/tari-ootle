//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::{BlockId, LeafBlock};
use tari_dan_common_types::{shard::Shard, Epoch, NodeHeight, ShardGroup};
use tari_dan_storage::{
    consensus_models::{Block, StateTransitionId, SubstateRecord},
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
};

use crate::{
    helpers::{create_block_with_qc, create_rocksdb, gen_substates},
    TEST_NUM_PRESHARDS,
};

#[test]
fn rocksdb() {
    let (db, _tmp) = create_rocksdb();
    operations(db);
}

fn operations(db: impl StateStore) {
    let num_transitions = 100; // Makes double
    const SHARD: Shard = Shard::first();
    let mut tx = db.create_write_tx().unwrap();

    let zero_block = Block::zero_block(Default::default(), TEST_NUM_PRESHARDS);
    zero_block.insert(&mut tx).unwrap();

    let substates = gen_substates(0..num_transitions, 0);
    let dummy_parent = LeafBlock {
        block_id: BlockId::zero(),
        height: NodeHeight(0),
        epoch: Epoch(0),
        shard_group: ShardGroup::all_shards(TEST_NUM_PRESHARDS),
    };
    for (key, value) in substates {
        let block = create_block_with_qc(&dummy_parent);
        tx.substates_create(&SubstateRecord::new(
            key,
            value.version(),
            value.into_substate_value(),
            SHARD,
            Epoch(0),
            *zero_block.id(),
            block.justify().calculate_id(),
        ))
        .unwrap();
    }

    // Add a couple for a different shard
    let substates = gen_substates(num_transitions..num_transitions + 2, 0);
    for (key, value) in substates {
        let block = create_block_with_qc(&dummy_parent);
        tx.substates_create(&SubstateRecord::new(
            key,
            value.version(),
            value.into_substate_value(),
            Shard::from(2),
            Epoch(0),
            *zero_block.id(),
            block.justify().calculate_id(),
        ))
        .unwrap();
    }

    let substates = gen_substates(0..num_transitions, 1);
    let dummy_parent = LeafBlock {
        block_id: BlockId::zero(),
        height: NodeHeight(10000),
        epoch: Epoch(1000),
        shard_group: ShardGroup::all_shards(TEST_NUM_PRESHARDS),
    };
    for (key, value) in substates {
        let block = create_block_with_qc(&dummy_parent);
        tx.substates_create(&SubstateRecord::new(
            key,
            value.version(),
            value.into_substate_value(),
            SHARD,
            Epoch(1000),
            *zero_block.id(),
            block.justify().calculate_id(),
        ))
        .unwrap();
    }

    let last_id = tx.state_transitions_get_last_id(SHARD).unwrap();
    assert_eq!(last_id.shard(), SHARD);
    assert_eq!(last_id.seq(), u64::from(num_transitions) * 2);
    assert_eq!(last_id.epoch(), Epoch(1000));

    let transitions = tx.state_transitions_get_n_after(10000, last_id, Epoch(1000)).unwrap();
    assert_eq!(transitions.len(), 0);

    let prev_id = StateTransitionId::new(Epoch(0), SHARD, 0);
    let transitions = tx.state_transitions_get_n_after(10000, prev_id, Epoch(1000)).unwrap();
    assert_eq!(transitions.len(), 100);

    let prev_id = StateTransitionId::new(Epoch(1000), SHARD, last_id.seq() - 10);
    let transitions = tx.state_transitions_get_n_after(10000, prev_id, Epoch(1001)).unwrap();
    for (i, transition) in transitions.iter().enumerate() {
        assert_eq!(transition.id.shard(), SHARD);
        assert_eq!(transition.id.epoch(), Epoch(1000));
        assert_eq!(transition.id.seq(), last_id.seq() - 10 + i as u64 + 1);
    }
    assert_eq!(transitions.len(), 10);

    let prev_seq = num_transitions - 10;
    let id = StateTransitionId::new(Epoch(0), SHARD, u64::from(prev_seq));

    let transitions = tx.state_transitions_get_n_after(10000, id, Epoch(100)).unwrap();
    assert_eq!(transitions.len(), 10);
}
