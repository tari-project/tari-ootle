//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A replica commits a block only with that block's state changes applied.
//!
//! The 3-chain rule commits a certified block on every replica, whether or not that replica voted for it. A replica
//! that has already voted at a block's height (for example for a sibling block a leader sent it alone) does not vote
//! for the block, but must still execute it and keep its change set, so that committing it applies its state. A
//! replica that does not hold a block's change set for any other reason must not commit the block at all: it reports
//! that it needs to sync instead.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use tari_consensus::{hotstuff::HotstuffEvent, messages::HotstuffMessage};
use tari_consensus_types::{BlockId, Decision, LastVoted};
use tari_engine_types::{commit_result::AbortReason, substate::SubstateId};
use tari_ootle_common_types::{
    Epoch,
    NodeHeight,
    SubstateLockType,
    SubstateVersion,
    VersionedSubstateIdRef,
    optional::Optional,
};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StorageError,
    consensus_models::{Block, BookkeepingModel, TransactionExecution, TransactionRecord},
};
use tari_ootle_transaction::TransactionId;
use tari_state_store_rocksdb::column_families::finalized_transaction::FinalizedTransactionLinkCf;

use crate::support::{
    ExecuteSpec,
    MessageFilter,
    Test,
    TestAddress,
    TestVnDestination,
    Validator,
    logging::setup_logger,
};

/// Heights past the target's leaf at which the target is made to hold a vote it did not cast for the committee's
/// block.
const VOTED_WINDOW: u64 = 8;

#[derive(Default)]
struct Observed {
    /// Heights of the target's votes.
    vote_heights: Vec<NodeHeight>,
    /// Heights of proposals carrying transactions that were delivered to the target.
    transaction_proposal_heights: BTreeSet<NodeHeight>,
}

fn observe_target(target: TestAddress, observed: Arc<Mutex<Observed>>) -> MessageFilter {
    Box::new(move |from, to, msg| {
        if *from == target {
            if let HotstuffMessage::Vote(vote) = msg {
                observed.lock().unwrap().vote_heights.push(vote.vote.block_height);
            }
            return true;
        }
        if *to == target &&
            let HotstuffMessage::Proposal(proposal) = msg &&
            proposal.block.commands().iter().any(|cmd| cmd.transaction().is_some())
        {
            observed
                .lock()
                .unwrap()
                .transaction_proposal_heights
                .insert(proposal.block.height());
        }
        true
    })
}

fn has_finalized_link(vn: &Validator, transaction_id: &TransactionId) -> bool {
    let tx = vn.state_store.create_read_tx().unwrap();
    tx.db()
        .cf(FinalizedTransactionLinkCf)
        .unwrap()
        .exists(transaction_id, "test")
        .unwrap()
}

fn finalized_execution(vn: &Validator, transaction_id: &TransactionId) -> Option<TransactionExecution> {
    vn.state_store
        .with_read_tx(|tx| TransactionExecution::get_finalized(tx, transaction_id))
        .optional()
        .unwrap()
}

fn is_committed(vn: &Validator, block_id: &BlockId) -> bool {
    vn.state_store
        .with_read_tx(|tx| {
            Ok::<_, StorageError>(Block::record_exists(tx, block_id)? && Block::get(tx, block_id)?.is_committed())
        })
        .unwrap()
}

/// Builds a transaction that commits on every validator except `target`, whose executor aborts it.
fn transaction_the_target_aborts(test: &Test, target: &TestAddress) -> TransactionRecord {
    let (transaction, inputs) = test.build_transaction(1);
    let spec = |decision| ExecuteSpec {
        transaction: transaction.transaction().clone(),
        decision,
        fee: 1,
        input_locks: inputs
            .iter()
            .map(|input| (input.substate_id().clone(), SubstateLockType::Write))
            .collect(),
        new_outputs: vec![],
        validator_fee_withdrawals: vec![],
        calls_template: None,
        fee_intent_fallback_fee: None,
    };
    for vn in test.validators_iter() {
        let decision = if vn.address == *target {
            Decision::Abort(AbortReason::ExecutionFailure)
        } else {
            Decision::Commit
        };
        test.add_execution_at_destination(TestVnDestination::Address(vn.address.clone()), spec(decision));
    }
    transaction
}

/// The substates `vn`'s committed execution of `transaction_id` put up.
fn committed_ups_of(vn: &Validator, transaction_id: &TransactionId) -> Vec<(SubstateId, SubstateVersion)> {
    let execution = finalized_execution(vn, transaction_id)
        .filter(|e| e.decision().is_commit())
        .unwrap_or_else(|| panic!("{} did not commit {transaction_id}", vn.address));
    execution
        .result()
        .finalize
        .any_accept()
        .expect("a committed transaction has an accepted diff")
        .up_iter()
        .map(|(id, substate)| (id.clone(), substate.version()))
        .collect()
}

fn holds_all(vn: &Validator, substates: &[(SubstateId, SubstateVersion)]) -> bool {
    vn.state_store
        .with_read_tx(|tx| {
            substates.iter().try_fold(true, |holds, (id, version)| {
                Ok::<_, StorageError>(holds && tx.substates_any_exist(Some(VersionedSubstateIdRef::new(id, *version)))?)
            })
        })
        .unwrap()
}

/// The committed block on `vn`'s chain whose commands include `transaction_id`.
fn committed_block_carrying(vn: &Validator, transaction_id: &TransactionId) -> BlockId {
    let leaf = vn.get_leaf_block();
    vn.state_store
        .with_read_tx(|tx| tx.blocks_get_all_between(leaf.epoch(), NodeHeight::zero(), leaf.height(), false, 1000))
        .unwrap()
        .into_iter()
        .find(|block| {
            block.is_committed() &&
                block
                    .commands()
                    .iter()
                    .any(|cmd| cmd.transaction().is_some_and(|atom| atom.id() == transaction_id))
        })
        .map(|block| *block.id())
        .unwrap_or_else(|| panic!("{} committed no block carrying {transaction_id}", vn.address))
}

/// Makes `vn` hold a vote at `height`, as a replica does once it has voted for a block at that height.
fn hold_vote_at(vn: &Validator, epoch: Epoch, height: NodeHeight) {
    vn.state_store
        .with_write_tx(|tx| {
            let held = LastVoted::get(&**tx, epoch).optional()?;
            if held.is_some_and(|held| held.height() >= height) {
                return Ok(());
            }
            LastVoted {
                block_id: BlockId::zero(),
                height,
                epoch,
            }
            .set(tx)
        })
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replica_executes_the_committees_blocks_at_heights_it_has_voted() {
    setup_logger();

    let target = TestAddress::new("4");
    let observed = Arc::new(Mutex::new(Observed::default()));

    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(120))
        .modify_consensus_constants(|c| {
            c.pacemaker_block_time = Duration::from_secs(2);
            c.missed_proposal_suspend_threshold = 50;
        })
        .with_message_filter(observe_target(target.clone(), observed.clone()))
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;

    test.start_epoch(Epoch(1)).await;

    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;
        if committed_height >= NodeHeight(4) {
            break;
        }
    }

    let leaf = test.get_validator(&target).get_leaf_block();
    let window_end = leaf.height() + NodeHeight(VOTED_WINDOW);
    log::info!("🎯 {target} holds votes up to height {window_end}");
    hold_vote_at(test.get_validator(&target), leaf.epoch(), window_end);

    let mut transaction_ids = Vec::new();
    for _ in 0..4 {
        let (transaction, _, _) = test.send_transaction_to_all(Decision::Commit, 1, 1, 1).await;
        transaction_ids.push(*transaction.id());
    }

    let mut network_max = NodeHeight::zero();
    loop {
        let (_, _, _, committed_height) = test.on_block_committed().await;
        network_max = network_max.max(committed_height);
        let target_vn = test.get_validator(&target);
        let target_leaf = target_vn.get_leaf_block().height();
        if target_leaf <= window_end {
            // A vote the target casts concurrently with the write above may overwrite it.
            hold_vote_at(target_vn, leaf.epoch(), window_end);
        }

        let voted_past_window = observed.lock().unwrap().vote_heights.iter().any(|h| *h > window_end);
        let all_finalized = transaction_ids
            .iter()
            .all(|id| test.validators_iter().all(|vn| has_finalized_link(vn, id)));
        if (voted_past_window && all_finalized) || network_max > window_end + NodeHeight(20) {
            break;
        }
    }

    let (transaction_proposal_heights, vote_heights) = {
        let observed = observed.lock().unwrap();
        (
            observed.transaction_proposal_heights.clone(),
            observed.vote_heights.clone(),
        )
    };
    assert!(
        transaction_proposal_heights
            .iter()
            .any(|h| *h > leaf.height() && *h <= window_end),
        "test premise: a block carrying transactions must fall in the voted window ({} ..= {window_end}), transaction \
         proposals were at {transaction_proposal_heights:?}",
        leaf.height(),
    );

    let target_vn = test.get_validator(&target);
    for id in &transaction_ids {
        assert!(
            test.validators_iter().all(|vn| has_finalized_link(vn, id)),
            "{id} was not finalized by every validator"
        );
        let execution = finalized_execution(target_vn, id);
        assert!(
            execution.as_ref().is_some_and(|e| e.decision().is_commit()),
            "{target} finalized {id} without its committed execution (found {:?})",
            execution.map(|e| e.decision())
        );
        assert!(
            target_vn.has_committed_substates(id),
            "{target} finalized {id} without committing its outputs"
        );
    }
    assert!(
        vote_heights.iter().any(|h| *h > window_end),
        "{target} never voted after the window ending at {window_end} (network height {network_max})"
    );

    test.assert_clean_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replica_does_not_commit_a_block_it_did_not_execute() {
    setup_logger();

    let target = TestAddress::new("4");

    let mut test = Test::builder()
        .with_test_timeout(Duration::from_secs(120))
        .modify_consensus_constants(|c| {
            c.pacemaker_block_time = Duration::from_secs(2);
            c.missed_proposal_suspend_threshold = 50;
        })
        .add_committee(0, vec!["1", "2", "3", "4"])
        .start()
        .await;

    // The target's executor aborts the transaction the rest of the committee commits, so the target declines to vote
    // for the block that carries it, and holds no change set for that block once the committee certifies it.
    let transaction = transaction_the_target_aborts(&test, &target);
    let transaction_id = *transaction.id();

    test.start_epoch(Epoch(1)).await;
    test.send_transaction_to_destination(TestVnDestination::All, transaction)
        .await;

    let honest = ["1", "2", "3"].map(TestAddress::new);
    let mut target_sync_required = false;
    let mut carrying_block = None;
    let mut commits_after_finalized = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        let (address, event) = tokio::time::timeout_at(deadline, test.on_hotstuff_event())
            .await
            .expect("timed out waiting for the committee to finalize the transaction");
        match event {
            HotstuffEvent::SyncRequired { .. } if address == target => target_sync_required = true,
            HotstuffEvent::Failure { message } => panic!("[{address}] Consensus failure: {message}"),
            HotstuffEvent::BlockCommitted { .. } => {
                if carrying_block.is_none() &&
                    honest
                        .iter()
                        .all(|addr| has_finalized_link(test.get_validator(addr), &transaction_id))
                {
                    carrying_block = Some(committed_block_carrying(
                        test.get_validator(&honest[0]),
                        &transaction_id,
                    ));
                }
                if carrying_block.is_some() {
                    commits_after_finalized += 1;
                }
            },
            _ => {},
        }
        if let Some(block_id) = carrying_block {
            let target_committed = is_committed(test.get_validator(&target), &block_id);
            if target_committed || (target_sync_required && commits_after_finalized >= 6) {
                break;
            }
            assert!(
                commits_after_finalized < 60,
                "{target} neither committed the block carrying {transaction_id} nor reported that it needs to sync"
            );
        }
    }
    let carrying_block = carrying_block.expect("loop exits once the carrying block is known");

    let mut committed_ups = Vec::new();
    for addr in &honest {
        let ups = committed_ups_of(test.get_validator(addr), &transaction_id);
        if committed_ups.is_empty() {
            committed_ups = ups;
        }
    }
    assert!(
        !committed_ups.is_empty(),
        "test premise: the transaction writes substates"
    );

    let target_vn = test.get_validator(&target);
    assert!(
        !is_committed(target_vn, &carrying_block) || holds_all(target_vn, &committed_ups),
        "{target} committed the block carrying {transaction_id} without its state changes"
    );
    assert!(
        !has_finalized_link(target_vn, &transaction_id) ||
            finalized_execution(target_vn, &transaction_id).is_some_and(|e| e.decision().is_commit()),
        "{target} finalized {transaction_id} without the committed execution"
    );
    assert!(
        target_sync_required,
        "{target} did not report that it needs to sync after declining a block the committee certified"
    );

    test.assert_clean_shutdown().await;
}
