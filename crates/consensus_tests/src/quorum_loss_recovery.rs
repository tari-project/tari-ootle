//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A committee that cannot reach quorum keeps timing out the same view: the view a NEWVIEW names comes from
//! certificates and the committee's timeout votes, never from how many times a member timed out. Once a quorum
//! is reachable again its members name the same view whatever they sent before, form the timeout certificate and
//! resume.

use std::time::Duration;

use tari_consensus_types::{Decision, LastSentNewView};
use tari_ootle_common_types::{Epoch, NodeHeight, optional::Optional};
use tari_ootle_storage::{StateStore, consensus_models::BookkeepingModel};

use crate::support::{Test, TestAddress, TestVnDestination, logging::setup_logger};

const OFFLINE: [&str; 2] = ["3", "4"];
const ONLINE: [&str; 2] = ["1", "2"];

/// Long enough for several leader timeouts at the block time below.
const PARTITION: Duration = Duration::from_secs(25);

async fn start() -> Test {
    Test::builder()
        .with_test_timeout(Duration::from_secs(240))
        .modify_consensus_constants(|config| {
            config.pacemaker_block_time = Duration::from_millis(200);
        })
        // n = 4, f = 1, quorum = 3: taking 2 nodes offline leaves 2 online (< quorum).
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await
}

fn last_sent_new_view(test: &Test, name: &str) -> Option<NodeHeight> {
    test.get_validator(&TestAddress::new(name))
        .state_store()
        .with_read_tx(|tx| LastSentNewView::get(tx, Epoch(1)))
        .optional()
        .unwrap()
        .map(|last_sent| last_sent.height)
}

async fn partition(test: &mut Test) -> NodeHeight {
    test.start_epoch(Epoch(1)).await;
    let (_, _, _, baseline_committed) = test.on_block_committed().await;
    log::info!("baseline committed at height {baseline_committed}");

    for addr in OFFLINE {
        test.network().go_offline(TestAddress::new(addr)).await;
    }
    tokio::time::sleep(PARTITION).await;

    // Drain the commit notifications buffered before quorum was lost, recording the highest pre-heal commit.
    let mut pre_heal_committed = baseline_committed;
    while let Ok((_, _, _, h)) = tokio::time::timeout(Duration::from_millis(100), test.on_block_committed()).await {
        pre_heal_committed = pre_heal_committed.max(h);
    }
    pre_heal_committed
}

async fn heal_and_commit(test: &mut Test, pre_heal_committed: NodeHeight) {
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn committee_resumes_after_a_long_quorum_loss() {
    setup_logger();
    let mut test = start().await;
    let pre_heal_committed = partition(&mut test).await;

    for name in ONLINE {
        let current_height = test.get_validator(&TestAddress::new(name))._current_view.get_height();
        assert_eq!(
            last_sent_new_view(&test, name),
            Some(current_height + NodeHeight(2)),
            "validator {name} must keep timing out the view after its current one"
        );
    }

    heal_and_commit(&mut test, pre_heal_committed).await;
    test.stop();
    test.assert_clean_shutdown().await;
}
