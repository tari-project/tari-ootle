//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Regression test for #3731: a committee that loses quorum for more than `MAX_VOTE_VIEW_LOOKAHEAD` (20)
//! consecutive leader timeouts must still form a timeout certificate and resume committing blocks once a
//! quorum of validators becomes reachable again.

use std::time::Duration;

use tari_consensus_types::{Decision, LastSentNewView};
use tari_ootle_common_types::{Epoch, NodeHeight, optional::Optional};
use tari_ootle_storage::{StateStore, consensus_models::BookkeepingModel};

use crate::support::{Test, TestAddress, TestVnDestination, logging::setup_logger};

const OFFLINE: [&str; 2] = ["3", "4"];
const ONLINE: [&str; 2] = ["1", "2"];

/// Minimum drift between `LastSentNewView` and `leaf.height()` before healing, strictly exceeding the
/// 20-view proposal vote lookahead window.
const MIN_DRIFT_PAST_WINDOW: NodeHeight = NodeHeight(23);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn committee_resumes_after_quorum_loss_exceeds_lookahead_window() {
    setup_logger();
    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(240))
        .modify_consensus_constants(|config| {
            config.pacemaker_block_time = Duration::from_millis(200);
        })
        // n = 4, f = 1, quorum = 3: taking 2 nodes offline leaves 2 online (< quorum).
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;

    test.start_epoch(Epoch(1)).await;
    let (_, _, _, baseline_committed) = test.on_block_committed().await;
    log::info!("baseline committed at height {baseline_committed}");

    for addr in OFFLINE {
        test.network().go_offline(TestAddress::new(addr)).await;
    }

    // Wait until every online validator has persisted LastSentNewView strictly past leaf + 23 (> 20-view window).
    loop {
        let all_past_window = ONLINE.iter().all(|&name| {
            let v = test.get_validator(&TestAddress::new(name));
            let leaf_height = v.get_leaf_block().height();
            let last_sent = v
                .state_store()
                .with_read_tx(|tx| LastSentNewView::get(tx, Epoch(1)))
                .optional()
                .unwrap()
                .map(|l| l.height)
                .unwrap_or(NodeHeight::zero());
            last_sent >= leaf_height.saturating_add(MIN_DRIFT_PAST_WINDOW)
        });
        if all_past_window {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Drain any commit notifications that were buffered before quorum was lost, recording the highest pre-heal commit.
    let mut pre_heal_committed = baseline_committed;
    while let Ok((_, _, _, h)) = tokio::time::timeout(Duration::from_millis(100), test.on_block_committed()).await {
        if h > pre_heal_committed {
            pre_heal_committed = h;
        }
    }

    // Heal the partition and submit a fresh transaction to verify full consensus + execution recovery.
    for addr in OFFLINE {
        test.network().go_online(&TestAddress::new(addr)).await;
    }
    let (tx, _, _) = test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
    test.wait_for_transaction_seen(TestVnDestination::All, tx.id()).await;

    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;
        if committed_height > pre_heal_committed && test.validators_iter().all(|v| v.has_committed_substates(tx.id())) {
            log::info!("committee resumed and committed post-heal tx at height {committed_height}");
            break;
        }
    }

    test.stop();
    test.assert_clean_shutdown().await;
}
