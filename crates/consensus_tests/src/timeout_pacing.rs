//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Every member counts the committee's timeout votes, so the committee moves past a view whose leader never forms
//! the timeout certificate, and a member that missed those votes joins the view the others moved on to.
//!
//! Every NEWVIEW for the first view the committee times out is dropped. That view's leader therefore receives none
//! of its votes: it never certifies the view and cannot see it time out. The other two live members count each
//! other's timeout votes and the leader's, see the view time out and move to the next one. The leader stays behind
//! until it joins them on the strength of their two votes, and with one member offline the committee needs all
//! three live members for a quorum.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use tari_consensus::messages::HotstuffMessage;
use tari_consensus_types::Decision;
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::support::{MessageFilter, Test, TestAddress, logging::setup_logger};

const OFFLINE: &str = "4";

/// Drops every NEWVIEW for the first view the committee times out.
fn drop_first_timeout_view_new_views(dropped_view: Arc<AtomicU64>, num_dropped: Arc<AtomicUsize>) -> MessageFilter {
    Box::new(move |_from, _to, msg| {
        let HotstuffMessage::NewView(new_view) = msg else {
            return true;
        };
        let height = new_view.timeout.height.as_u64();
        let _ = dropped_view.compare_exchange(0, height, Ordering::SeqCst, Ordering::SeqCst);
        if height != dropped_view.load(Ordering::SeqCst) {
            return true;
        }
        num_dropped.fetch_add(1, Ordering::SeqCst);
        false
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_committee_moves_past_a_view_its_leader_never_certifies() {
    setup_logger();
    let dropped_view = Arc::new(AtomicU64::new(0));
    let num_dropped = Arc::new(AtomicUsize::new(0));
    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(120))
        .modify_consensus_constants(|config| {
            config.pacemaker_block_time = Duration::from_millis(200);
        })
        .with_message_filter(drop_first_timeout_view_new_views(
            dropped_view.clone(),
            num_dropped.clone(),
        ))
        // n = 4, f = 1, quorum = 3: with one member offline every live member is needed for a quorum.
        .add_committee(0, vec!["1", "2", "3", OFFLINE])
        .start()
        .await;

    test.network().go_offline(TestAddress::new(OFFLINE)).await;
    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
    }
    test.start_epoch(Epoch(1)).await;

    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;
        let dropped_view = NodeHeight(dropped_view.load(Ordering::SeqCst));
        if !dropped_view.is_zero() &&
            num_dropped.load(Ordering::SeqCst) > 0 &&
            committed_height > dropped_view + NodeHeight(1)
        {
            log::info!("committed {committed_height} past the timed-out view {dropped_view}");
            break;
        }
    }

    test.stop();
    test.assert_clean_shutdown().await;
}
