//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A catch-up page's continuation cursor is only a position to resume from: no certificate backs it.
//!
//! Every catch-up page sent to the replica under test is preceded by a page with the same request id that carries no
//! blocks and names an unreachable continuation height, as a faulty committee member answering the replica's request
//! would send. The test requires that the replica still proposes in its turns while the committee keeps committing.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tari_consensus::messages::{CatchUpPageMessage, HotstuffMessage};
use tari_consensus_types::Decision;
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::support::{MessageFilter, MessageInjector, Test, TestAddress, logging::setup_logger};

const COMMITTED_HEIGHT_TARGET: NodeHeight = NodeHeight(16);

fn replica() -> TestAddress {
    TestAddress::new("1")
}

/// Ahead of each catch-up page to the replica, delivers a page from the same sender answering the same request with
/// no blocks and a continuation at the greatest height.
fn forge_unreachable_continuations() -> MessageInjector {
    let replica = replica();
    Box::new(move |from, to, msg| {
        if *to != replica {
            return Vec::new();
        }
        let HotstuffMessage::CatchUpPage(page) = msg else {
            return Vec::new();
        };
        let mut forged = CatchUpPageMessage::empty(page.request_id, page.epoch);
        forged.next_height = Some(NodeHeight(u64::MAX));
        vec![(from.clone(), HotstuffMessage::CatchUpPage(Box::new(forged)))]
    })
}

fn count_replica_proposals(proposals: Arc<AtomicUsize>) -> MessageFilter {
    let replica = replica();
    Box::new(move |from, _to, msg| {
        if *from == replica && matches!(msg, HotstuffMessage::Proposal(_)) {
            proposals.fetch_add(1, Ordering::Relaxed);
        }
        true
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_page_continuation_does_not_stop_a_replica_proposing() {
    setup_logger();

    let proposals = Arc::new(AtomicUsize::new(0));
    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(120))
        .modify_consensus_constants(|c| {
            c.pacemaker_block_time = Duration::from_secs(2);
            c.missed_proposal_suspend_threshold = 50;
        })
        .with_message_filter(count_replica_proposals(proposals.clone()))
        .with_message_injector(forge_unreachable_continuations())
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;

    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
    }
    test.start_epoch(Epoch(1)).await;

    let mut committed = NodeHeight::zero();
    while committed < COMMITTED_HEIGHT_TARGET {
        let (_, _, _, height) = tokio::time::timeout(Duration::from_secs(60), test.on_block_committed())
            .await
            .unwrap_or_else(|_| panic!("no block committed within 60s (committed height {committed})"));
        committed = committed.max(height);
    }

    assert!(
        proposals.load(Ordering::Relaxed) > 0,
        "replica {} sent no proposal while the committee committed up to height {committed}",
        replica()
    );

    test.stop();
    test.assert_clean_shutdown().await;
}
