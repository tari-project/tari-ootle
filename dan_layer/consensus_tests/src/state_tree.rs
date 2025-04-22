//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_consensus::hotstuff::HotStuffError;
use tari_dan_common_types::{Epoch, NodeHeight};
use tari_dan_storage::{
    consensus_models::{Decision, StateTransitionId},
    StateStore,
    StateStoreReadTransaction,
};
use tari_state_tree::{key_mapper::SpreadPrefixKeyMapper, memory_store::MemoryTreeStore};

use crate::support::{logging::setup_logger, Test, TestAddress, TEST_NUM_PRESHARDS};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn check_state_transitions() {
    setup_logger();
    let mut test = Test::builder()
        .modify_consensus_constants(|config| {
            config.pacemaker_block_time = Duration::from_millis(500);
        })
        .add_committee(0, vec!["1"])
        .start()
        .await;
    let _ignore = test.send_transaction_to_all(Decision::Commit, 100, 1, 10).await;
    let _ignore = test.send_transaction_to_all(Decision::Commit, 200, 1, 1).await;
    let _ignore = test.send_transaction_to_all(Decision::Commit, 1, 1, 1).await;
    test.start_epoch(Epoch(1)).await;

    loop {
        test.on_block_committed().await;

        if test.is_transaction_pool_empty() {
            break;
        }
        let leaf = test.get_validator(&TestAddress::new("1")).get_leaf_block();
        if leaf.height >= NodeHeight(10) {
            panic!("Not all transaction committed after {} blocks", leaf.height);
        }
    }
    test.start_epoch(Epoch(2)).await;
    loop {
        let (_, _, epoch, height) = test.on_block_committed().await;

        if epoch == Epoch(2) {
            break;
        }
        if height >= NodeHeight(10) {
            panic!("Not all transaction committed after {} blocks", height);
        }
    }

    test.get_validator(&TestAddress::new("1"))
        .state_store
        .with_read_tx(|tx| {
            let checkpoint = tx.epoch_checkpoint_get(Epoch(1)).unwrap();

            for shard in TEST_NUM_PRESHARDS.all_shards_iter() {
                let id = StateTransitionId::new(Epoch(0), shard, 0);
                let transitions = tx.state_transitions_get_n_after(1000, id, Epoch(2)).unwrap();

                let shard_root = checkpoint.get_shard_root(shard);
                // No state changes
                if shard_root.iter().all(|x| *x == 0) {
                    assert_eq!(transitions.len(), 0, "Shard {} should have no state transitions", shard);
                } else {
                    assert!(!transitions.is_empty(), "Shard {} should have state transitions", shard);
                }

                let mut store = MemoryTreeStore::new();
                let mut tree = tari_state_tree::StateTree::<_, SpreadPrefixKeyMapper>::new(&mut store);
                let values = transitions.iter().map(|transition| transition.to_tree_change());
                let root = tree.put_substate_changes(None, 1, values).unwrap();
                assert_eq!(root, shard_root, "Shard {} root hash mismatch", shard);
            }

            Ok::<_, HotStuffError>(())
        })
        .unwrap();
    test.assert_clean_shutdown().await;
}
