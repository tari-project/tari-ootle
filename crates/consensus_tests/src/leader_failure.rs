//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::{Duration, Instant};

use ootle_byte_type::ToByteType;
use tari_consensus::hotstuff::HotStuffError;
use tari_consensus_types::Decision;
use tari_crypto::tari_utilities::ByteArray;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::{StateStore, StateStoreReadTransaction};

use crate::support::{helpers, logging::setup_logger, Test, TestAddress, TestVnDestination};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_shard_node_goes_down() {
    setup_logger();
    let mut test = Test::builder()
        // Allow enough time for leader failures
        .with_test_timeout(Duration::from_secs(60))
        .modify_consensus_constants(|config_mut| {
            config_mut.missed_proposal_suspend_threshold = 10;
            config_mut.missed_proposal_evict_threshold = 10;
            config_mut.pacemaker_block_time = Duration::from_secs(5);
        })
        .add_committee(0, vec!["1", "2", "3", "4", "5"])
        .start()
        .await;

    let failure_node = TestAddress::new("4");

    let mut tx_ids = Vec::with_capacity(10);
    for _ in 0..10 {
        let (tx, _, _) = test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
        tx_ids.push(*tx.id());
    }

    // Take the VN offline - if we do it in the loop below, all transactions may have already been finalized (local
    // only) by committed block 1
    log::info!("😴 {failure_node} is offline");
    test.network().go_offline(failure_node.clone()).await;

    test.start_epoch(Epoch(1)).await;

    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;

        if committed_height == NodeHeight(1) {
            // This allows a few more leader failures to occur
            test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
            test.wait_for_pool_count(TestVnDestination::All, 1).await;
        }

        if test.validators_iter().filter(|vn| vn.address != failure_node).all(|v| {
            let c = v.get_transaction_pool_count();
            log::info!("{} has {} transactions in pool", v.address, c);
            c == 0
        }) {
            break;
        }

        if committed_height > NodeHeight(50) {
            panic!("Not all transaction committed after {} blocks", committed_height);
        }
    }

    test.stop();

    test.validators_iter()
        .filter(|vn| vn.address != failure_node)
        .for_each(|v| {
            tx_ids.iter().for_each(|tx_id| {
                assert!(
                    v.has_committed_substates(tx_id),
                    "Validator {} did not commit",
                    v.address
                );
            });
        });

    log::info!("total messages sent: {}", test.network().total_messages_sent());
    test.assert_clean_shutdown_except(&[failure_node]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_shard_neighbour_nodes_go_down() {
    // "neighbour" meaning next to each other in the leader order
    setup_logger();
    let mut test = Test::builder()
        // Allow enough time for leader failures
        .with_test_timeout(Duration::from_secs(60))
        .modify_consensus_constants(|config_mut| {
            config_mut.missed_proposal_suspend_threshold = 10;
            config_mut.missed_proposal_evict_threshold = 10;
            config_mut.pacemaker_block_time = Duration::from_secs(5);
        })
        // For f = 2 we need 7 nodes
        .add_committee(0, vec!["1", "2", "3", "4", "5", "6", "7"])
        .start()
        .await;

    let failure_node1 = TestAddress::new("4");
    let failure_node2 = TestAddress::new("5");

    let mut tx_ids = Vec::with_capacity(10);
    for _ in 0..10 {
        let (tx, _, _) = test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
        tx_ids.push(*tx.id());
    }

    // Take the VN offline - if we do it in the loop below, all transactions may have already been finalized (local
    // only) by committed block 1
    log::info!("😴 {failure_node1} is offline");
    log::info!("😴 {failure_node2} is offline");
    test.network().go_offline(failure_node1.clone()).await;
    test.network().go_offline(failure_node2.clone()).await;

    test.start_epoch(Epoch(1)).await;

    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;

        if committed_height == NodeHeight(1) {
            // This allows a few more leader failures to occur
            test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
            test.wait_for_pool_count(TestVnDestination::All, 1).await;
        }

        if test
            .validators_iter()
            .filter(|vn| vn.address != failure_node1 && vn.address != failure_node2)
            .all(|v| {
                let c = v.get_transaction_pool_count();
                log::info!("{} has {} transactions in pool", v.address, c);
                c == 0
            })
        {
            break;
        }

        if committed_height > NodeHeight(50) {
            panic!("Not all transaction committed after {} blocks", committed_height);
        }
    }

    test.stop();

    test.validators_iter()
        .filter(|vn| vn.address != failure_node1 && vn.address != failure_node2)
        .for_each(|v| {
            tx_ids.iter().for_each(|tx_id| {
                assert!(
                    v.has_committed_substates(tx_id),
                    "Validator {} did not commit",
                    v.address
                );
            });
        });

    log::info!("total messages sent: {}", test.network().total_messages_sent());
    test.assert_clean_shutdown_except(&[failure_node1]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_shard_node_goes_down_and_gets_evicted() {
    setup_logger();
    let failure_node = TestAddress::new("4");

    let mut test = Test::builder()
        // Allow enough time for leader failures
        .with_test_timeout(Duration::from_secs(30))
        .modify_consensus_constants(|config_mut| {
            // The node will be evicted after three missed proposals
            config_mut.missed_proposal_suspend_threshold = 1;
            config_mut.missed_proposal_evict_threshold = 3;
            config_mut.pacemaker_block_time = Duration::from_secs(5);
        })
        .add_committee(0, vec!["1", "2", "3", "4", "5"])
        .add_failure_node(failure_node.clone())
        .start()
        .await;

    for _ in 0..10 {
        test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
    }

    // Take the VN offline - if we do it in the loop below, all transactions may have already been finalized (local
    // only) by committed block 1
    log::info!("😴 {failure_node} is offline");
    test.network().go_offline(failure_node.clone()).await;

    test.start_epoch(Epoch(1)).await;

    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;

        // Takes missed_proposal_evict_threshold * 5 (members) + 3 (chain) blocks for nodes to evict. So we need to keep
        // the transactions coming to speed up this test.
        if committed_height >= NodeHeight(1) && committed_height < NodeHeight(20) {
            // This allows a few more leader failures to occur
            test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
        }

        let eviction_proofs = test
            .validators()
            .get(&TestAddress::new("1"))
            .unwrap()
            .epoch_manager()
            .eviction_proofs()
            .await;
        if !eviction_proofs.is_empty() {
            for proof in &eviction_proofs {
                // Uncomment the following to dump the eviction proofs to files for fixtures
                // std::fs::write(
                //     format!("/tmp/eviction_proof_{}.json", proof.node_to_evict()),
                //     serde_json::to_string_pretty(proof).unwrap(),
                // )
                // .unwrap();
                // Check the proof is valid
                proof
                    .validate(4, &|pk| {
                        Ok(test
                            .validators()
                            .values()
                            .any(|vn| vn.public_key.as_bytes() == pk.as_bytes()))
                    })
                    .unwrap();
            }

            break;
        }

        if committed_height >= NodeHeight(40) {
            panic!("Not all transaction committed after {} blocks", committed_height);
        }
    }

    test.stop();

    // test.validators_iter()
    //     .filter(|vn| vn.address != failure_node)
    //     .for_each(|v| {
    //         assert!(v.has_committed_substates(), "Validator {} did not commit", v.address);
    //     });

    let (_, failure_node_pk) = helpers::derive_keypair_from_address(&failure_node);
    test.validators()
        .get(&TestAddress::new("1"))
        .unwrap()
        .state_store()
        .with_read_tx(|tx| {
            let leaf = tx.leaf_block_get(Epoch(1))?;
            assert!(
                tx.suspended_nodes_is_evicted(leaf.block_id(), &failure_node_pk.to_byte_type())
                    .unwrap(),
                "{failure_node} is not evicted"
            );
            Ok::<_, HotStuffError>(())
        })
        .unwrap();

    let eviction_proofs = test
        .validators()
        .get(&TestAddress::new("1"))
        .unwrap()
        .epoch_manager()
        .eviction_proofs()
        .await;
    for proof in &eviction_proofs {
        assert_eq!(proof.node_to_evict().as_bytes(), failure_node_pk.as_bytes());
    }

    // Epoch manager state is shared between all validators, so each working validator (4) should create a proof.
    // assert_eq!(eviction_proofs.len(), 4);

    log::info!("total messages sent: {}", test.network().total_messages_sent());
    test.assert_clean_shutdown_except(&[failure_node]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multi_shard_node_goes_down() {
    // Although leader failure does not generally affect the other shards, we test that the foreign proposal commit
    // proof still validates with dummy blocks included
    setup_logger();
    let mut test = Test::builder()
        // Allow enough time for leader failures
        .with_test_timeout(Duration::from_secs(60))
        .modify_consensus_constants(|config_mut| {
            config_mut.missed_proposal_suspend_threshold = 10;
            config_mut.missed_proposal_evict_threshold = 10;
            config_mut.pacemaker_block_time = Duration::from_secs(5);
        })
        .add_committee(0, vec!["1", "2", "3", "4", "5"])
        .add_committee(1, vec!["6", "7"])
        .start()
        .await;

    let failure_node = TestAddress::new("4");

    let mut tx_ids = Vec::with_capacity(10);
    for _ in 0..10 {
        let (tx, _, _) = test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
        tx_ids.push(*tx.id());
    }

    // Take the VN offline - if we do it in the loop below, all transactions may have already been finalized (local
    // only) by committed block 1
    log::info!("😴 {failure_node} is offline");
    test.network().go_offline(failure_node.clone()).await;

    test.start_epoch(Epoch(1)).await;

    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;

        if committed_height == NodeHeight(1) {
            // This allows a few more leader failures to occur
            test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
            test.wait_for_pool_count(TestVnDestination::All, 1).await;
        }

        if test.validators_iter().filter(|vn| vn.address != failure_node).all(|v| {
            let c = v.get_transaction_pool_count();
            log::info!("{} has {} transactions in pool", v.address, c);
            c == 0
        }) {
            break;
        }

        if committed_height > NodeHeight(50) {
            panic!("Not all transaction committed after {} blocks", committed_height);
        }
    }

    test.stop();

    // TODO: assert something - transactions are not guaranteed to involve all shard groups
    // test.validators_iter()
    //     .filter(|vn| vn.address != failure_node)
    //     .for_each(|v| {
    //         tx_ids.iter().for_each(|tx_id| {
    //             assert!(
    //                 v.has_committed_substates(tx_id),
    //                 "Validator {} did not commit",
    //                 v.address
    //             );
    //         });
    //     });

    log::info!("total messages sent: {}", test.network().total_messages_sent());
    test.assert_clean_shutdown_except(&[failure_node]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_shard_node_goes_down_and_catches_up() {
    setup_logger();
    let mut test = Test::builder()
        // Allow enough time for leader failures
        .with_test_timeout(Duration::from_secs(60))
        .modify_consensus_constants(|constants_mut| {
            constants_mut.missed_proposal_suspend_threshold = 10;
            constants_mut.missed_proposal_evict_threshold = 10;
            constants_mut.pacemaker_block_time = Duration::from_secs(5);
        })
        .modify_config(|config_mut| {
            config_mut.enable_eviction_proposal = false;
        })
        .add_committee(0, vec!["1", "2", "3", "4", "5"])
        .start()
        .await;

    let failure_node = TestAddress::new("4");

    let mut tx_ids = Vec::with_capacity(12);
    for _ in 0..10 {
        let (tx, _, _) = test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
        tx_ids.push(*tx.id());
    }

    test.start_epoch(Epoch(1)).await;
    let epoch_start = Instant::now();
    let mut is_back_online = false;
    let mut had_gone_offline = false;

    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;

        if !had_gone_offline && epoch_start.elapsed() >= Duration::from_secs(2) {
            log::info!("😴 {failure_node} is offline");
            test.network().go_offline(failure_node.clone()).await;
            let (tx, _, _) = test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
            tx_ids.push(*tx.id());
            had_gone_offline = true;
        }

        if !is_back_online && epoch_start.elapsed() >= Duration::from_secs(13) {
            log::info!("🚀 {failure_node} is online again");
            test.network().go_online(&failure_node).await;
            let (tx, _, _) = test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
            is_back_online = true;
            tx_ids.push(*tx.id());
        }

        if committed_height == NodeHeight(1) {
            // This allows a few more leader failures to occur
            test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
            test.wait_for_pool_count(TestVnDestination::All, 1).await;
        }

        if is_back_online &&
            test.validators_iter()
                .all(|v| tx_ids.iter().all(|tx_id| v.has_committed_substates(tx_id)))
        {
            break;
        }

        if committed_height > NodeHeight(50) {
            panic!("Not all transaction committed after {} blocks", committed_height);
        }
    }

    test.stop();

    log::info!("total messages sent: {}", test.network().total_messages_sent());
    test.assert_clean_shutdown_except(&[failure_node]).await;
}
