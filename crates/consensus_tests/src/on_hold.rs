//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_consensus::hotstuff::ConsensusCurrentState;
use tari_consensus_types::Decision;
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::support::{Test, TestAddress, logging::setup_logger};

/// Round-trip test: Running → OnHold → (released) → Idle/CheckSync/Running.
///
/// Boots a single-committee network, runs a transaction to drive each validator into `Running`,
/// then requests on-hold and asserts the state machine reaches `OnHold`, releases the hold and
/// asserts the state transitions back to `Running` via the Idle/CheckSync path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn on_hold_round_trip() {
    setup_logger();
    let mut test = Test::builder().add_committee(0, vec!["1", "2"]).start().await;
    let (tx1, _, _) = test.send_transaction_to_all(Decision::Commit, 1, 1, 1).await;
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
    test.assert_all_validators_committed(tx1.id());

    // Every validator should be in Running at this point.
    for vn in test.validators().values() {
        wait_for_state(vn, ConsensusCurrentState::Running, Duration::from_secs(5)).await;
    }

    // 1. Request on-hold.
    for vn in test.validators().values() {
        vn.request_on_hold_and_wait(Duration::from_secs(5)).await;
        assert_eq!(vn.current_state_machine_state(), ConsensusCurrentState::OnHold);
    }

    // 2. Release on-hold and assert the state machine transitions all the way back to Running.
    // Route: OnHold → Idle → CheckSync → Running (verified in logs; here we just assert end state).
    for vn in test.validators().values() {
        vn.release_on_hold_and_wait(Duration::from_secs(5)).await;
        wait_for_state(vn, ConsensusCurrentState::Running, Duration::from_secs(10)).await;
    }

    test.stop();
    test.assert_clean_shutdown().await;
}

async fn wait_for_state(vn: &crate::support::Validator, target: ConsensusCurrentState, timeout: Duration) {
    let mut rx = vn.current_state_machine_state.clone();
    let fut = async {
        while *rx.borrow() != target {
            rx.changed().await.expect("state machine dropped");
        }
    };
    tokio::time::timeout(timeout, fut)
        .await
        .unwrap_or_else(|_| panic!("validator {} did not reach {}", vn.address, target));
}
