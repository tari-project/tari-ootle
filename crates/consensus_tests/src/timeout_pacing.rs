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
    collections::HashSet,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use tari_consensus::messages::HotstuffMessage;
use tari_consensus_types::Decision;
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::support::{MessageFilter, Test, TestAddress, logging::setup_logger};

const LIVE: [&str; 3] = ["1", "2", "3"];
const OFFLINE: &str = "4";
/// A live member that does not lead the first view the committee times out.
const LOSSY: &str = "1";

/// The first view the committee times out, and how many messages for it were dropped.
#[derive(Clone, Default)]
struct DroppedView {
    height: Arc<AtomicU64>,
    num_dropped: Arc<AtomicUsize>,
}

impl DroppedView {
    /// Whether `height` is the dropped view, taking it as the dropped view if none is yet.
    fn is(&self, height: NodeHeight) -> bool {
        let _ = self
            .height
            .compare_exchange(0, height.as_u64(), Ordering::SeqCst, Ordering::SeqCst);
        height.as_u64() == self.height.load(Ordering::SeqCst)
    }

    fn drop(&self) -> bool {
        self.num_dropped.fetch_add(1, Ordering::SeqCst);
        false
    }
}

/// Drops every NEWVIEW for the first view the committee times out.
fn drop_new_views(dropped: DroppedView) -> MessageFilter {
    Box::new(move |_from, _to, msg| match msg {
        HotstuffMessage::NewView(new_view) if dropped.is(new_view.timeout.height) => dropped.drop(),
        _ => true,
    })
}

/// Drops every NEWVIEW for the first view the committee times out, and the first timeout vote for it that `LOSSY`
/// sends to each member. `LOSSY` alone then counts the view out, and the vote it sent for it reached nobody.
fn drop_new_views_and_lossy_first_votes(dropped: DroppedView) -> MessageFilter {
    let lost_to = Mutex::new(HashSet::<TestAddress>::new());
    Box::new(move |from, to, msg| match msg {
        HotstuffMessage::NewView(new_view) if dropped.is(new_view.timeout.height) => {
            assert_ne!(
                to.as_str(),
                LOSSY,
                "the lossy member must not lead the dropped view, or it never counts the view out"
            );
            dropped.drop()
        },
        HotstuffMessage::TimeoutVote(vote)
            if dropped.is(vote.height) && from.as_str() == LOSSY && lost_to.lock().unwrap().insert(to.clone()) =>
        {
            dropped.drop()
        },
        _ => true,
    })
}

async fn commits_past_the_dropped_view(filter: impl FnOnce(DroppedView) -> MessageFilter) {
    setup_logger();
    let dropped = DroppedView::default();
    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(120))
        .modify_consensus_constants(|config| {
            config.pacemaker_block_time = Duration::from_millis(200);
        })
        .with_message_filter(filter(dropped.clone()))
        // n = 4, f = 1, quorum = 3: with one member offline every live member is needed for a quorum.
        .add_committee(0, vec![LIVE[0], LIVE[1], LIVE[2], OFFLINE])
        .start()
        .await;

    test.network().go_offline(TestAddress::new(OFFLINE)).await;
    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
    }
    test.start_epoch(Epoch(1)).await;

    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;
        let dropped_view = NodeHeight(dropped.height.load(Ordering::SeqCst));
        if !dropped_view.is_zero() &&
            dropped.num_dropped.load(Ordering::SeqCst) > 0 &&
            committed_height > dropped_view + NodeHeight(1)
        {
            log::info!("committed {committed_height} past the timed-out view {dropped_view}");
            break;
        }
    }

    test.stop();
    test.assert_clean_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_committee_moves_past_a_view_its_leader_never_certifies() {
    commits_past_the_dropped_view(drop_new_views).await;
}

/// One member counts the view out while its own vote for the view reached nobody. It sends no vote for that view
/// again, so the other two can only count the view out once it answers the votes they keep sending for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn members_left_short_of_a_quorum_are_answered_by_one_that_moved_on() {
    commits_past_the_dropped_view(drop_new_views_and_lossy_first_votes).await;
}
