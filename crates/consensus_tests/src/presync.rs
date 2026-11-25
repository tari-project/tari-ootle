//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::{Duration, Instant};

use tari_consensus::hotstuff::ConsensusCurrentState;
use tari_ootle_common_types::Epoch;
use tokio::time::sleep;

use crate::support::{Test, TestAddress, logging::setup_logger};

/// Waits until `addr` has presynced `count` times and settled back in `Idle`, then checks that it presyncs no more
/// while the epoch stays the same.
async fn wait_for_presyncs(test: &Test, addr: &TestAddress, count: usize) {
    let vn = test.get_validator(addr);
    let deadline = Instant::now() + Duration::from_secs(30);
    while vn.sync_manager.num_presyncs() < count || vn.current_state_machine_state() != ConsensusCurrentState::Idle {
        assert!(
            Instant::now() < deadline,
            "{addr} presynced {} time(s), expected {count}; state {}",
            vn.sync_manager.num_presyncs(),
            vn.current_state_machine_state()
        );
        sleep(Duration::from_millis(50)).await;
    }
    sleep(Duration::from_secs(1)).await;
    assert_eq!(
        vn.sync_manager.num_presyncs(),
        count,
        "{addr} presynced again within the same epoch"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pending_validator_presyncs_once_per_epoch_until_it_activates() {
    setup_logger();
    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(60))
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;
    let pending = TestAddress::new("4");
    test.get_validator(&pending)
        .epoch_manager
        .set_activation_epoch(Epoch(3));

    test.start_epoch(Epoch(1)).await;
    wait_for_presyncs(&test, &pending, 1).await;

    test.start_epoch(Epoch(2)).await;
    wait_for_presyncs(&test, &pending, 2).await;

    test.start_epoch(Epoch(3)).await;
    let vn = test.get_validator(&pending);
    assert!(
        vn.current_state_machine_state().is_running(),
        "{pending} did not join consensus at its activation epoch"
    );
    assert_eq!(
        vn.sync_manager.num_presyncs(),
        2,
        "{pending} presynced after it activated"
    );

    test.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_presync_that_finds_no_checkpoint_is_retried_in_the_same_epoch() {
    setup_logger();
    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(60))
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;
    let pending = TestAddress::new("4");
    let vn = test.get_validator(&pending);
    vn.epoch_manager.set_activation_epoch(Epoch(3));
    vn.sync_manager.report_checkpoint_unavailable_on_next_presync();

    test.start_epoch(Epoch(1)).await;
    wait_for_presyncs(&test, &pending, 2).await;

    test.stop();
}
