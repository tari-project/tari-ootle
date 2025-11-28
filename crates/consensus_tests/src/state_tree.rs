//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_consensus::hotstuff::HotStuffError;
use tari_consensus_types::Decision;
use tari_ootle_common_types::{optional::Optional, Epoch, NodeHeight};
use tari_ootle_storage::{consensus_models::SubstateValueFilterFlags, StateStore, StateStoreReadTransaction};
use tari_state_tree::{
    key_mapper::SpreadPrefixKeyMapper,
    memory_store::MemoryTreeStore,
    SPARSE_MERKLE_PLACEHOLDER_HASH,
};

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
    let _ignore = test.send_transaction_to_all(Decision::Commit, 100, 1, 10).await;
    test.start_epoch(Epoch(1)).await;

    loop {
        test.on_block_committed().await;

        if test.is_transaction_pool_empty() {
            break;
        }
        let leaf = test.get_validator(&TestAddress::new("1")).get_leaf_block();
        if leaf.height >= NodeHeight(10) {
            panic!("Not all transactions committed after {} blocks", leaf.height);
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

    test.stop();

    test.get_validator(&TestAddress::new("1"))
        .state_store
        .with_read_tx(|tx| {
            let checkpoint = tx
                .epoch_checkpoint_get_all_from_epoch(Epoch(1), 1)
                .unwrap()
                .pop()
                .unwrap();

            for shard in TEST_NUM_PRESHARDS.all_shards_iter() {
                let mut all_transitions = vec![];
                let mut next_state_version = 1;
                while let Some(transitions) = tx
                    .state_transitions_get_starting_at(shard, next_state_version, SubstateValueFilterFlags::all())
                    .optional()
                    .unwrap()
                {
                    if transitions.epoch > checkpoint.epoch() {
                        break;
                    }

                    next_state_version = transitions.state_version + 1;
                    all_transitions.push(transitions);
                }
                log::info!(
                    "Shard {}: Found {} transitions until state version {}",
                    shard,
                    all_transitions.len(),
                    next_state_version
                );

                let shard_root = checkpoint.get_shard_root(shard);
                // No state changes
                if shard_root == SPARSE_MERKLE_PLACEHOLDER_HASH {
                    assert!(
                        all_transitions.is_empty(),
                        "Shard {} should have no state transitions",
                        shard
                    );
                } else {
                    assert!(
                        !all_transitions.is_empty(),
                        "Shard {} should have state transitions",
                        shard
                    );
                }

                let mut store = MemoryTreeStore::new();
                let mut tree = tari_state_tree::StateTree::<_, SpreadPrefixKeyMapper>::new(&mut store);
                let values = all_transitions
                    .iter()
                    .flat_map(|t| &t.updates)
                    .map(|transition| transition.to_tree_change());
                let root = tree.put_substate_changes(None, 1, values).unwrap();
                assert_eq!(root, shard_root, "Shard {} root hash mismatch", shard);
            }

            Ok::<_, HotStuffError>(())
        })
        .unwrap();
    test.assert_clean_shutdown().await;
}
