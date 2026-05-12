//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::{Duration, Instant};

use tari_consensus::hotstuff::{HotStuffError, HotstuffEvent};
use tari_consensus_types::{Decision, LeafBlock};
use tari_ootle_common_types::{Epoch, NodeHeight, optional::Optional};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    consensus_models::{Block, BookkeepingModel},
};
use tokio::sync::broadcast;

use crate::support::{Test, TestAddress, logging::setup_logger};

async fn epoch_change(mut test: Test) {
    test.start_epoch(Epoch(1)).await;
    let mut remaining_txs = 10;

    loop {
        if remaining_txs > 0 {
            test.send_transaction_to_all(Decision::Commit, 1, 5, 1).await;
            remaining_txs -= 1;
        }
        if remaining_txs == 5 {
            test.start_epoch(Epoch(2)).await;
        }

        if remaining_txs == 0 && test.is_all_submitted_transactions_finalized() {
            break;
        }

        let (_, _, _, height) = test.on_block_committed().await;
        if height > NodeHeight(30) {
            panic!("Not all transaction committed after {} blocks", height);
        }
    }

    let vns = test.validators_iter();
    let mut histories = vec![];
    // Assert epoch changed
    for vn in vns {
        vn.state_store
            .with_read_tx(|tx| {
                let leaf_block = tx.leaf_block_get(Epoch(2))?;
                assert_eq!(leaf_block.epoch(), Epoch(2));

                Ok::<_, HotStuffError>(())
            })
            .unwrap();

        histories.push(vn.transaction_executor.get_history());
    }

    // Check that all transactions executed consistently
    assert!(!histories.is_empty());
    for history in &histories {
        for (tx_id, execution) in history {
            for inner_history in &histories {
                if std::ptr::eq(history, inner_history) {
                    continue;
                }
                let other_exec = inner_history.get(tx_id).expect("missing execution for tx_id");
                let r1 = execution.execution.result();
                let r2 = other_exec.execution.result();
                assert_eq!(r1.finalize.result.is_accept(), r2.finalize.result.is_accept());

                // NB: executions use a consistent epoch
                assert_eq!(
                    execution.execution_epoch, other_exec.execution_epoch,
                    "epoch mismatch for tx_id {}",
                    tx_id
                );
            }
        }
    }

    test.stop();
    test.assert_clean_shutdown().await;
    log::info!("total messages sent: {}", test.network().total_messages_sent());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_shard_epoch_change() {
    setup_logger();
    let test = Test::builder()
        .modify_config(|config_mut| {
            config_mut.epoch_end_grace_period = Duration::from_millis(10);
        })
        .add_committee(0, vec!["1", "2"])
        .start()
        .await;

    epoch_change(test).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multishard_epoch_change() {
    setup_logger();
    let test = Test::builder()
        .modify_config(|config_mut| {
            config_mut.epoch_end_grace_period = Duration::from_millis(0);
        })
        .add_committee(0, vec!["1", "2"])
        .add_committee(1, vec!["3", "4"])
        .start()
        .await;

    epoch_change(test).await;
}

/// Reproduces the wedge described in `consensus-epoch-change-race-cond`:
///
/// One validator's base-layer oracle is still behind when consensus commits the EndEpoch
/// block. `process_end_of_epoch` then asks the local epoch manager for `next_epoch`'s hash to
/// stamp into the new genesis; the lookup fails with `NoEpochFound`. Before the fix, that error
/// killed the worker and left the node permanently stuck on the old epoch — even after the
/// oracle eventually caught up, no code path retried the deferred work. After the fix, the
/// work is parked in `pending_end_of_epoch`, the worker keeps running, and the next
/// `EpochChanged` event (or the next worker startup) retries the transition.
///
/// The test:
///   1. Runs four validators in a single committee.
///   2. Caps validator "1"'s oracle at `Epoch(1)` so `get_epoch_hash(Epoch(2))` returns `NoEpochFound`. (The cap
///      intentionally does NOT override `current_epoch()` — the vote-time check uses that, and we want the chain to
///      commit the EOE on every node so `process_end_of_epoch` actually runs and trips the failure mode.)
///   3. Drives an epoch change to `Epoch(2)`. All four nodes vote on the EOE, the chain commits it via 3-chain, and
///      `process_end_of_epoch` runs on each. Three succeed and advance to Epoch(2); validator "1" defers instead of
///      crashing.
///   4. Asserts the other three have advanced to `Epoch(2)` while validator "1"'s pacemaker is still on `Epoch(1)` with
///      a committed EOE block in its chain.
///   5. Clears the lag and refires `EpochChanged(2)` (modeling the oracle catching up).
///   6. Asserts validator "1" then advances to `Epoch(2)`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn epoch_change_with_lagging_oracle() {
    setup_logger();
    let mut test = Test::builder()
        .modify_config(|cfg| {
            cfg.epoch_end_grace_period = Duration::from_millis(10);
        })
        .modify_consensus_constants(|c| {
            c.pacemaker_block_time = Duration::from_secs(2);
        })
        .with_test_timeout(Duration::from_secs(60))
        // 4 validators so 3 honest votes-yes is enough for quorum without the lagged one.
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;

    let lagging_addr = TestAddress::new("1");

    test.start_epoch(Epoch(1)).await;

    // A few transactions to get the chain moving in epoch 1.
    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 1, 1).await;
    }

    // Cap the lagged validator's oracle BEFORE the epoch transition. After this,
    // `get_epoch_hash(Epoch(2))` returns `NoEpochFound` for this validator only.
    test.get_validator(&lagging_addr)
        .epoch_manager
        .set_oracle_visible_epoch(Epoch(1));

    // Trigger the epoch change. All four workers receive `EpochChanged(2)` and set
    // `next_epoch`. Voting on the EOE block goes through normally (we deliberately do
    // not cap `current_epoch()` so the lagged node still votes Yes), so quorum is
    // reached and the chain commits the EOE on every node via the 3-chain rule. This
    // is exactly the state the production bug ends up in once a lagged node catches
    // up via sync — the EOE is committed locally, but `process_end_of_epoch` then
    // tries to look up `next_epoch`'s hash and fails.
    test.start_epoch(Epoch(2)).await;

    // Drive the chain so the EOE 3-chain commits and the non-lagged validators run
    // process_end_of_epoch successfully (creating the next epoch's genesis and advancing
    // their pacemakers to Epoch(2)). Sending more transactions keeps the leaders proposing.
    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 1, 1).await;
    }

    // Wait until the three non-lagged validators have advanced to Epoch(2). Don't include
    // the lagged validator — under the fix it stays on Epoch(1) until the oracle catches up.
    wait_for_validators_at_epoch(&mut test, &lagging_addr, Epoch(2), Duration::from_secs(30)).await;

    // Confirm the bug condition is reproduced on the lagged validator:
    //   (a) its pacemaker is still on Epoch(1)
    //   (b) the EOE block is committed in its chain (process_end_of_epoch was reached)
    //   (c) it has no leaf in Epoch(2) (next genesis was deferred)
    let lagged = test.get_validator(&lagging_addr);
    assert_eq!(
        lagged._current_view.get_epoch(),
        Epoch(1),
        "lagged validator's pacemaker must remain on Epoch(1) before recovery"
    );

    let has_committed_eoe = lagged
        .state_store
        .with_read_tx(|tx| chain_has_committed_epoch_end(tx, Epoch(1)))
        .unwrap();
    assert!(
        has_committed_eoe,
        "lagged validator should have committed the EOE block via 3-chain (process_end_of_epoch must have run and \
         deferred)"
    );

    let leaf_at_2 = lagged
        .state_store
        .with_read_tx(|tx| LeafBlock::get(tx, Epoch(2)))
        .optional()
        .unwrap();
    assert!(
        leaf_at_2.is_none(),
        "lagged validator must NOT have a leaf in Epoch(2) before the oracle catches up; got {leaf_at_2:?}"
    );

    log::info!("✅ bug condition reproduced: lagged validator stuck at Epoch(1) with committed EOE");

    // Now simulate the oracle catching up: clear the lag and re-publish `EpochChanged(2)`.
    // The worker's `on_epoch_manager_event` sees `has_pending_end_of_epoch` and calls
    // `try_resume_pending_end_of_epoch`, which re-runs `process_end_of_epoch`. With the
    // oracle now current, `get_epoch_hash(Epoch(2))` succeeds, the next genesis is created
    // and the pacemaker advances.
    let lagged_sg = lagged.shard_group;
    lagged.epoch_manager.clear_oracle_lag();
    test.get_validator_mut(&lagging_addr)
        .epoch_manager
        .set_current_epoch(Epoch(2), lagged_sg)
        .await;

    // Assert recovery: the lagged validator now reaches Epoch(2).
    wait_for_single_validator_at_epoch(&mut test, &lagging_addr, Epoch(2), Duration::from_secs(15)).await;

    log::info!("✅ recovery confirmed: lagged validator advanced to Epoch(2) after oracle caught up");

    test.stop();
    test.assert_clean_shutdown().await;
}

/// Walks the chain from the leaf of `epoch` looking for a committed `EndEpoch` block.
fn chain_has_committed_epoch_end<TTx: StateStoreReadTransaction>(
    tx: &TTx,
    epoch: Epoch,
) -> Result<bool, HotStuffError> {
    let leaf = LeafBlock::get(tx, epoch)?;
    if leaf.is_genesis() {
        return Ok(false);
    }
    let mut block = Block::get(tx, leaf.block_id())?;
    loop {
        if block.is_epoch_end() && block.is_committed() {
            return Ok(true);
        }
        if block.height().is_zero() || block.parent().is_zero() {
            return Ok(false);
        }
        block = block.get_parent(tx)?;
    }
}

/// Waits until every validator EXCEPT `excluded` reaches `epoch` on its pacemaker.
async fn wait_for_validators_at_epoch(test: &mut Test, excluded: &TestAddress, epoch: Epoch, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        // Drain block events so receiver buffers don't overflow.
        let _unused = tokio::time::timeout(Duration::from_millis(100), test.on_block_committed()).await;

        let all_advanced = test
            .validators_iter()
            .filter(|v| v.address != *excluded)
            .all(|v| v._current_view.get_epoch() >= epoch);
        if all_advanced {
            return;
        }
    }
    panic!("Timed out waiting for non-excluded validators to reach {epoch}");
}

/// Waits until `addr`'s pacemaker reaches `epoch`.
async fn wait_for_single_validator_at_epoch(test: &mut Test, addr: &TestAddress, epoch: Epoch, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let _unused = tokio::time::timeout(Duration::from_millis(100), test.on_block_committed()).await;

        if test.get_validator(addr)._current_view.get_epoch() >= epoch {
            return;
        }
    }
    panic!(
        "Timed out waiting for {addr} to reach {epoch} (currently on {})",
        test.get_validator(addr)._current_view.get_epoch()
    );
}

/// Reproduces the second variant of the epoch-change wedge described in the
/// `consensus-epoch-change-eoe-no-vote-wedge` investigation. PR #2104 fixed the case where a
/// validator manages to vote on the EOE but its oracle is lagging when
/// `process_end_of_epoch` looks up the next epoch's hash. This test covers the *other* entry
/// point: a validator whose oracle is lagging at vote-time, so it no-votes the EOE with
/// `NoVoteReason::NotEndOfEpoch`. Peers form quorum without it, commit, and roll over.
///
/// In production this wedges the validator permanently — its leaf stays in the old epoch,
/// catch-up sync fails because peers' leaves are now in the next epoch and refuse cross-epoch
/// requests, and the oracle catching up has no code path to retry the missed transition.
///
/// The fix gates a `NeedsSync` escalation on **cryptographic evidence**, not on wall-clock
/// heuristics: in `MessageBuffer::next`, any future-epoch Proposal/NewView whose embedded
/// 2f+1 QC validates against the future epoch's committee proves the network has rolled
/// over. The lagged validator escalates to `CheckSync` → `Syncing`, which in production
/// downloads the epoch checkpoint and resumes on the new epoch's genesis.
///
/// Test flow:
/// 1. Four validators, single committee. Start Epoch(1) and exchange a few transactions.
/// 2. Take validator "1" *network-offline* and cap its `current_epoch()` at Epoch(1). Taking it offline keeps it out of
///    the leader rotation as far as peers are concerned — the other three time-out validator "1"'s leadership slot and
///    form a TC, so the chain progresses without it. Capping `current_epoch()` ensures that *if* it ever did receive
///    the EOE proposal it would no-vote (the production failure mode).
/// 3. Trigger Epoch(2). The three honest validators commit the EOE and run `process_end_of_epoch` to advance to
///    Epoch(2). They keep proposing blocks in Epoch(2), each carrying a 2f+1 QC over Epoch(2).
/// 4. Bring validator "1" back online and clear the oracle cap (modeling a node whose network rejoined and whose
///    base-layer scanner caught up). The next Epoch(2) Proposal that the network delivers to it goes through
///    `MessageBuffer::next`, where the probe verifies the embedded QC against the Epoch(2) committee and raises
///    `HotStuffError::NeedsSync`.
/// 5. We assert on the resulting `HotstuffEvent::Failure` event — its message identifies the QC-based reason. No
///    time-based sleeps: the assertion fires deterministically when an authenticated Epoch(2) QC reaches the validator.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn epoch_change_no_vote_wedge_escalates_on_future_qc() {
    setup_logger();
    let mut test = Test::builder()
        .modify_config(|cfg| {
            cfg.epoch_end_grace_period = Duration::from_millis(10);
        })
        .modify_consensus_constants(|c| {
            // Short pacemaker so the honest validators time-out validator "1"'s leadership
            // slot quickly and form a TC to skip past it.
            c.pacemaker_block_time = Duration::from_secs(1);
        })
        .with_test_timeout(Duration::from_secs(60))
        // 4 validators so 3 honest yes-votes are enough for quorum without the lagged one.
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;

    let lagging_addr = TestAddress::new("1");

    test.start_epoch(Epoch(1)).await;

    // A few transactions to advance the chain in Epoch(1) before the boundary.
    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 1, 1).await;
    }

    // Cap validator "1"'s view of `current_epoch()`. Unlike `set_oracle_visible_epoch` (which
    // only caps `get_epoch_hash`), this makes the vote-time check
    // `em_epoch > current_epoch` fail for this validator — reproducing the no-vote path.
    test.get_validator(&lagging_addr)
        .epoch_manager
        .set_oracle_current_epoch_cap(Epoch(1));

    // Take validator "1" offline so the network delivers no messages to/from it. This stops
    // its (now-wedged) view from blocking leader rotation on the honest committee — peers
    // time out validator "1"'s slot, gather a TC, and the next leader takes over.
    test.network().go_offline(lagging_addr.clone()).await;

    // Trigger the epoch change. The shared `inner.current_epoch` moves to Epoch(2) and
    // `EpochChanged(2)` is broadcast (via tokio, not the test network — so even the offline
    // validator receives it). The three honest validators commit the EOE and advance.
    test.start_epoch(Epoch(2)).await;

    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 1, 1).await;
    }

    // The three honest validators advance to Epoch(2); the lagged one (offline + capped)
    // cannot make progress and remains on Epoch(1).
    wait_for_validators_at_epoch(&mut test, &lagging_addr, Epoch(2), Duration::from_secs(45)).await;

    let lagged = test.get_validator(&lagging_addr);
    assert_eq!(
        lagged._current_view.get_epoch(),
        Epoch(1),
        "lagged validator's view must remain on Epoch(1) before recovery"
    );
    let leaf_at_2 = lagged
        .state_store
        .with_read_tx(|tx| LeafBlock::get(tx, Epoch(2)))
        .optional()
        .unwrap();
    assert!(
        leaf_at_2.is_none(),
        "lagged validator must NOT have a leaf in Epoch(2) before recovery; got {leaf_at_2:?}"
    );

    log::info!("✅ wedge reproduced: validator {lagging_addr} stuck on Epoch(1) while peers committed Epoch(2)");

    // Subscribe to validator "1"'s event stream BEFORE clearing the oracle cap and going
    // online, so we don't miss the Failure event the worker publishes when it raises
    // `HotStuffError::NeedsSync`.
    let mut events = lagged.events.resubscribe();

    // Oracle catches up, and the network rejoins. From now on, the honest validators'
    // Epoch(2) proposals reach validator "1" — each carries an authenticated 2f+1 QC over
    // Epoch(2). The first one trips the probe in `MessageBuffer::next`.
    lagged.epoch_manager.clear_oracle_current_epoch_cap();
    test.network().go_online(&lagging_addr).await;

    // Honest validators keep producing proposals in Epoch(2); send more transactions to
    // drive them.
    for _ in 0..3 {
        test.send_transaction_to_all(Decision::Commit, 1, 1, 1).await;
    }

    // Wait for the Failure event whose message identifies the QC-based escalation. This is
    // deterministic — fires on the first authenticated Epoch(2) QC that arrives.
    let outcome = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match events.recv().await {
                Ok(HotstuffEvent::Failure { message }) if message.contains("Received valid 2f+1 QC") => {
                    return message;
                },
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => panic!("validator event channel closed"),
            }
        }
    })
    .await
    .expect("Timed out waiting for QC-based NeedsSync escalation");

    log::info!("✅ probe fired: {outcome}");
    assert!(
        outcome.contains("Epoch(2)"),
        "escalation reason should reference the next-epoch QC; got: {outcome}"
    );
}
