//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A replica that misses consecutive proposals catches up as soon as a later proposal shows it is behind.
//!
//! When a replica misses the proposals for a few heights, every later proposal justifies a block it does not
//! hold. Those proposals are classified "future" and buffered, and the blocks they need never arrive through the
//! buffer, so the only other way back is three leader timeouts. The test drops three consecutive proposals to one
//! replica, then requires it to reach the height the next delivered proposal justifies within two leader timeouts
//! of receiving it: well short of the three a timeout-driven recovery needs, with headroom for slow CI runners.

use std::{
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use tari_consensus::messages::HotstuffMessage;
use tari_consensus_types::Decision;
use tari_ootle_common_types::{Epoch, NodeHeight};

use crate::support::{Test, TestAddress, logging::setup_logger};

/// Consecutive proposals withheld from the target.
const DROPPED_PROPOSALS: u64 = 3;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replica_behind_a_proposal_catches_up_without_leader_timeouts() {
    setup_logger();

    let block_time = Duration::from_secs(2);
    // A leader timeout is the block time plus a delta of at least 2s plus 2s of assumed latency. Waiting for
    // three of them takes three times this.
    let leader_timeout = block_time + Duration::from_secs(4);
    let catch_up_bound = leader_timeout * 2;

    let target = TestAddress::new("4");
    // First height of the withheld window; 0 until armed.
    let drop_from = Arc::new(AtomicU64::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    // When the target received the first proposal above the window, and the height that proposal justifies.
    let delivered = Arc::new(Mutex::new(None::<(Instant, NodeHeight)>));

    let drop_from_f = drop_from.clone();
    let dropped_f = dropped.clone();
    let delivered_f = delivered.clone();
    let target_f = target.clone();

    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(120))
        .modify_consensus_constants(|c| {
            c.pacemaker_block_time = block_time;
            c.missed_proposal_suspend_threshold = 50;
        })
        .with_message_filter(Box::new(move |from, to, msg| {
            let start = drop_from_f.load(Ordering::SeqCst);
            if start == 0 || *to != target_f || *from == target_f {
                return true;
            }
            let HotstuffMessage::Proposal(proposal) = msg else {
                return true;
            };
            let height = proposal.block.height().as_u64();
            if (start..start + DROPPED_PROPOSALS).contains(&height) {
                dropped_f.fetch_add(1, Ordering::SeqCst);
                return false;
            }
            if height >= start + DROPPED_PROPOSALS {
                let mut delivered = delivered_f.lock().unwrap();
                if delivered.is_none() {
                    *delivered = Some((Instant::now(), proposal.block.justify().height()));
                }
            }
            true
        }))
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;

    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
    }
    test.start_epoch(Epoch(1)).await;

    let mut network_max = NodeHeight::zero();
    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;
        network_max = network_max.max(committed_height);
        let target_leaf = test.get_validator(&target).get_leaf_block().height();

        let delivered = *delivered.lock().unwrap();
        if drop_from.load(Ordering::SeqCst) == 0 {
            if network_max >= NodeHeight(5) && target_leaf + NodeHeight(3) >= network_max {
                let start = network_max + NodeHeight(3);
                log::info!("🎯 Withholding proposals {start}..+{DROPPED_PROPOSALS} from {target}");
                drop_from.store(start.as_u64(), Ordering::SeqCst);
            }
        } else if let Some((delivered_at, justify_height)) = delivered &&
            target_leaf >= justify_height
        {
            let elapsed = delivered_at.elapsed();
            log::info!("✅ {target} reached {justify_height} {elapsed:.2?} after the first proposal ahead of it");
            assert!(
                elapsed < catch_up_bound,
                "{target} took {elapsed:.2?} to reach {justify_height} after receiving a proposal justifying it, \
                 longer than two leader timeouts ({catch_up_bound:.2?})"
            );
            break;
        } else {
            // Still waiting for the withheld window to pass, or for the target to catch up.
        }

        assert!(
            network_max <= NodeHeight(60),
            "{target} never reached the justified height (network height {network_max}, target leaf {target_leaf})"
        );
    }

    assert!(
        dropped.load(Ordering::SeqCst) > 0,
        "test premise: proposals to the target must have been withheld"
    );
    test.stop();
    test.assert_clean_shutdown().await;
}
