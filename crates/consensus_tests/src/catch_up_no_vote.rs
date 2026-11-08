//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A replica does not vote on the blocks it imports through catch-up, but does receive its catch-up server's leaf as
//! a proposal.
//!
//! Catch-up imports blocks the committee has already certified, so a vote on one can never help form a certificate.
//! It can conflict with a vote the replica cast before it lost its voting records, e.g. one restored from a backup
//! snapshot. The server's leaf has no certificate yet, so it is sent as a proposal the replica may vote on. The test
//! withholds three consecutive proposals from one replica so that it imports them through catch-up, then requires
//! that it votes on none of them, that its server relays a leaf as a proposal, and that it votes on a later live
//! proposal.

use std::{
    collections::HashSet,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use ootle_byte_type::ToByteType;
use tari_consensus::messages::HotstuffMessage;
use tari_consensus_types::{BlockId, Decision, LastVoted};
use tari_ootle_common_types::{Epoch, NodeHeight, optional::Optional};
use tari_ootle_storage::{
    StateStore,
    StorageError,
    consensus_models::{Block, BookkeepingModel},
};
use tari_ootle_transaction::TransactionId;

use crate::support::{
    MessageFilter,
    NetworkSendObserver,
    Test,
    TestAddress,
    TestStore,
    TestVnDestination,
    helpers,
    logging::setup_logger,
};

/// Consecutive proposals withheld from the target.
const DROPPED_PROPOSALS: u64 = 3;

#[derive(Clone, Default)]
struct Observed {
    /// Every proposal delivered to the target, by sender.
    proposal_deliveries: HashSet<(TestAddress, BlockId)>,
    /// Proposals their proposers broadcast to the target and the filter withheld.
    withheld: HashSet<BlockId>,
    /// Withheld proposals that carried at least one transaction command.
    withheld_with_txs: HashSet<BlockId>,
    /// Transactions sequenced in withheld proposals.
    withheld_txs: HashSet<TransactionId>,
    /// Blocks the target received in catch-up responses.
    caught_up: HashSet<BlockId>,
    /// Proposals the target received other than as their proposer's broadcast, i.e. a catch-up server's leaf: from a
    /// validator other than the proposer, or a second time from the proposer.
    relayed: HashSet<BlockId>,
    /// The target's votes.
    votes: Vec<(BlockId, NodeHeight)>,
    /// Uncommitted withheld catch-up blocks that held a non-empty saved `BlockDiff` when the target next voted.
    withheld_with_saved_diff: HashSet<BlockId>,
    /// Whether `LastVoted` has been seeded over the withheld window before the target requests catch-up.
    seeded_last_voted: bool,
}

/// Records what the target sends and receives, and withholds from it the proposals broadcast in the window starting
/// at `drop_from` once that is armed. A relayed proposal is never withheld.
fn observe_target(target: TestAddress, drop_from: Arc<AtomicU64>, observed: Arc<Mutex<Observed>>) -> MessageFilter {
    Box::new(move |from, to, msg| {
        if *from == target {
            if let HotstuffMessage::Vote(vote) = msg {
                observed
                    .lock()
                    .unwrap()
                    .votes
                    .push((vote.vote.block_id, vote.vote.block_height));
            }
            return true;
        }
        if *to != target {
            return true;
        }
        match msg {
            HotstuffMessage::CatchUpSyncResponse(response) => {
                observed.lock().unwrap().caught_up.insert(*response.block.id());
                true
            },
            HotstuffMessage::Proposal(proposal) => {
                let block_id = *proposal.block.id();
                let mut observed = observed.lock().unwrap();
                let is_first_delivery = observed.proposal_deliveries.insert((from.clone(), block_id));
                let (_, sender_pk) = helpers::derive_keypair_from_address(from);
                if !is_first_delivery || *proposal.block.proposed_by() != sender_pk.to_byte_type() {
                    observed.relayed.insert(block_id);
                    return true;
                }
                let start = drop_from.load(Ordering::SeqCst);
                let height = proposal.block.height().as_u64();
                if start != 0 && (start..start + DROPPED_PROPOSALS).contains(&height) {
                    observed.withheld.insert(block_id);
                    for cmd in proposal.block.commands() {
                        if let Some(tx) = cmd.transaction() {
                            observed.withheld_with_txs.insert(block_id);
                            observed.withheld_txs.insert(*tx.id());
                        }
                    }
                    return false;
                }
                true
            },
            _ => true,
        }
    })
}

#[expect(clippy::too_many_lines)]
async fn run_catch_up_window(seed_last_voted_over_window: bool) {
    setup_logger();

    let target = TestAddress::new("4");
    // First height of the withheld window; 0 until armed.
    let drop_from = Arc::new(AtomicU64::new(0));
    let observed = Arc::new(Mutex::new(Observed::default()));
    let send_observer: NetworkSendObserver = {
        let target = target.clone();
        let drop_from = drop_from.clone();
        let observed = observed.clone();
        Arc::new(
            move |address: &TestAddress, store: &TestStore, message: &HotstuffMessage| {
                if *address != target {
                    return;
                }
                let mut obs = observed.lock().unwrap();
                let start = drop_from.load(Ordering::SeqCst);
                if seed_last_voted_over_window &&
                    !obs.seeded_last_voted &&
                    start != 0 &&
                    !obs.withheld.is_empty() &&
                    matches!(message, HotstuffMessage::CatchUpSyncRequest(_))
                {
                    let last_voted = LastVoted {
                        block_id: BlockId::zero(),
                        height: NodeHeight(start + DROPPED_PROPOSALS - 1),
                        epoch: Epoch(1),
                    };
                    store.with_write_tx(|tx| last_voted.set(tx)).unwrap();
                    obs.seeded_last_voted = true;
                }
                if !matches!(message, HotstuffMessage::Vote(_)) {
                    return;
                }
                let candidates = obs
                    .withheld_with_txs
                    .intersection(&obs.caught_up)
                    .copied()
                    .collect::<Vec<_>>();
                for id in candidates {
                    let has_uncommitted_diff = store
                        .with_read_tx(|tx| {
                            let Some(block) = Block::get(tx, &id).optional()? else {
                                return Ok::<_, StorageError>(false);
                            };
                            Ok(!block.is_committed() && !block.get_diff(tx)?.is_empty())
                        })
                        .unwrap();
                    if has_uncommitted_diff {
                        obs.withheld_with_saved_diff.insert(id);
                    }
                }
            },
        )
    };

    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(120))
        .modify_consensus_constants(|c| {
            c.pacemaker_block_time = Duration::from_secs(2);
            c.missed_proposal_suspend_threshold = 50;
            c.max_commands_in_block = 1;
        })
        .with_message_filter(observe_target(target.clone(), drop_from.clone(), observed.clone()))
        .with_send_observer(send_observer)
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;

    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
    }
    test.start_epoch(Epoch(1)).await;

    let mut network_max = NodeHeight::zero();
    let mut window_txs: Vec<TransactionId> = Vec::new();
    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;
        network_max = network_max.max(committed_height);
        let target_leaf = test.get_validator(&target).get_leaf_block().height();

        let start = drop_from.load(Ordering::SeqCst);
        if start == 0 {
            if network_max >= NodeHeight(5) && target_leaf + NodeHeight(3) >= network_max {
                test.network().pause();
                for _ in 0..6 {
                    let (tx_rec, _, _) = test.send_transaction_to_all(Decision::Commit, 1, 2, 1).await;
                    test.wait_for_transaction_seen(TestVnDestination::All, tx_rec.id())
                        .await;
                    window_txs.push(*tx_rec.id());
                }
                let max_leaf = test
                    .validators()
                    .values()
                    .map(|v| v.get_leaf_block().height())
                    .max()
                    .unwrap();
                let start = max_leaf + NodeHeight(2);
                log::info!("🎯 Withholding proposals {start}..+{DROPPED_PROPOSALS} from {target}");
                drop_from.store(start.as_u64(), Ordering::SeqCst);
                test.network().start();
            }
        } else {
            let after_window = NodeHeight(start + DROPPED_PROPOSALS);
            let voted_after_window = observed.lock().unwrap().votes.iter().any(|(_, h)| *h >= after_window);
            let window_txs_committed = window_txs
                .iter()
                .all(|tx_id| test.validators().values().all(|v| v.has_committed_substates(tx_id)));
            if target_leaf >= after_window && voted_after_window && window_txs_committed {
                break;
            }
        }

        assert!(
            network_max <= NodeHeight(60),
            "{target} never voted past the withheld window (network height {network_max}, target leaf {target_leaf})"
        );
    }

    {
        let observed = observed.lock().unwrap().clone();
        assert!(
            !observed.withheld.is_empty(),
            "test premise: proposals to the target must have been withheld"
        );
        assert!(
            !observed.withheld_txs.is_empty(),
            "test premise: withheld proposals must carry at least one window transaction"
        );
        let caught_up_withheld = observed
            .withheld_with_txs
            .intersection(&observed.caught_up)
            .copied()
            .collect::<Vec<_>>();
        assert!(
            !caught_up_withheld.is_empty(),
            "test premise: the target must have imported a withheld transaction block through catch-up"
        );
        if seed_last_voted_over_window {
            assert!(
                observed.seeded_last_voted,
                "test premise: LastVoted must have been seeded before the target requested catch-up"
            );
        }
        assert!(
            !observed.withheld_with_saved_diff.is_empty(),
            "caught-up transaction blocks {caught_up_withheld:?} had no non-empty BlockDiff saved before committing"
        );
        let votes_on_withheld = observed
            .votes
            .iter()
            .filter(|(id, _)| observed.withheld.contains(id))
            .collect::<Vec<_>>();
        assert!(
            votes_on_withheld.is_empty(),
            "{target} voted on blocks it imported through catch-up: {votes_on_withheld:?}"
        );
        assert!(
            !observed.relayed.is_empty(),
            "{target} caught up without its catch-up server sending its leaf as a proposal"
        );
    }

    for tx_id in &window_txs {
        test.assert_all_validators_committed(tx_id);
    }

    test.stop();
    test.assert_clean_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replica_does_not_vote_on_blocks_it_catches_up_on() {
    run_catch_up_window(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replica_saves_the_block_diff_of_catch_up_blocks_at_or_below_last_voted() {
    run_catch_up_window(true).await;
}
