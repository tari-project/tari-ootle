//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests that the input substates a shard group loads to execute its transactions are bounded per transaction and
//! per block, whatever the transactions pay.

use tari_common_types::types::PrivateKey;
use tari_consensus_types::Decision;
use tari_engine_types::commit_result::AbortReason;
use tari_ootle_common_types::{Epoch, InputDeclaration, NodeHeight, SubstateLockType, VersionedSubstateId};
use tari_ootle_storage::{
    StateStore,
    consensus_models::{Block, TransactionRecord},
};
use tari_ootle_transaction::Transaction;

use crate::support::{Test, TestAddress, TestVnDestination, build_transaction_from, logging::setup_logger};

const TEST_MAX_TRANSACTION_INPUT_LOAD_BYTES: usize = 128 * 1024;
const TEST_MAX_BLOCK_INPUT_LOAD_BYTES: usize = 128 * 1024;
const LARGE_COMPONENT_STATE_BYTES: usize = 100 * 1024;

async fn start_test() -> Test {
    setup_logger();
    Test::builder()
        .add_committee(0, vec!["1", "2"])
        .modify_consensus_constants(|constants| {
            constants.max_transaction_input_load_bytes = TEST_MAX_TRANSACTION_INPUT_LOAD_BYTES;
            constants.max_block_input_load_bytes = TEST_MAX_BLOCK_INPUT_LOAD_BYTES;
        })
        .start()
        .await
}

/// A local-only transaction that reads `inputs` and executes to `decision`.
async fn send_local_read(test: &mut Test, inputs: &[VersionedSubstateId], decision: Decision) -> TransactionRecord {
    let mut builder = Transaction::builder_localnet(Epoch(1));
    for input in inputs {
        builder = builder.add_input(InputDeclaration::read_versioned(
            input.substate_id().clone(),
            input.version(),
        ));
    }
    let tx = build_transaction_from(builder.build_and_seal(&PrivateKey::default()));
    let input_locks = inputs
        .iter()
        .map(|input| (input.substate_id().clone(), SubstateLockType::Read))
        .collect();
    test.create_execution_at_destination_for_transaction(TestVnDestination::All, &tx, decision, 5, input_locks, vec![]);
    test.send_transaction_to_destination(TestVnDestination::All, tx.clone())
        .await;
    tx
}

async fn run_until_pool_is_empty(test: &mut Test) {
    test.start_epoch(Epoch(1)).await;

    loop {
        test.on_block_committed().await;

        if test.is_transaction_pool_empty() {
            break;
        }

        let leaf = test.get_validator(&TestAddress::new("1")).get_leaf_block();
        if leaf.height > NodeHeight(60) {
            panic!("Not all transactions finalized after {} blocks", leaf.height);
        }
    }

    test.stop();
}

/// The number of transaction commands in each committed block of validator "1", in height order.
fn committed_transaction_commands_per_block(test: &Test) -> Vec<usize> {
    let vn = test.get_validator(&TestAddress::new("1"));
    let leaf = vn.get_leaf_block();
    vn.state_store()
        .with_read_tx(|tx| Block::get_all_blocks_between(tx, leaf.epoch, NodeHeight::zero(), leaf.height, false, 1000))
        .unwrap()
        .iter()
        .filter(|block| block.is_committed())
        .map(|block| {
            block
                .commands()
                .iter()
                .filter(|cmd| cmd.transaction().is_some())
                .count()
        })
        .collect()
}

/// No block could load two large values for one transaction, so the transaction is aborted without being executed,
/// whatever it would have paid.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transaction_loading_more_than_a_transaction_may_load_is_aborted() {
    let mut test = start_test().await;

    let large_inputs =
        test.create_substates_on_vns_with_state_size(TestVnDestination::All, 2, LARGE_COMPONENT_STATE_BYTES);
    let tx = send_local_read(&mut test, &large_inputs, Decision::Commit).await;

    run_until_pool_is_empty(&mut test).await;

    test.assert_all_validators_have_decision(tx.id(), Decision::Abort(AbortReason::InputLoadTooLarge))
        .await;
    test.assert_all_validators_did_not_commit(tx.id());
    test.assert_clean_shutdown().await;
}

/// Each transaction loads one large value. The leader spreads them over enough blocks that no block loads more than
/// the budget, and every transaction is still committed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn large_inputs_are_spread_over_blocks_within_the_budget() {
    const NUM_TRANSACTIONS: usize = 4;
    let mut test = start_test().await;

    let large_inputs = test.create_substates_on_vns_with_state_size(
        TestVnDestination::All,
        NUM_TRANSACTIONS,
        LARGE_COMPONENT_STATE_BYTES,
    );
    let mut transactions = Vec::with_capacity(NUM_TRANSACTIONS);
    for large in &large_inputs {
        let tx = send_local_read(&mut test, std::slice::from_ref(large), Decision::Commit).await;
        transactions.push(tx);
    }

    run_until_pool_is_empty(&mut test).await;

    for tx in &transactions {
        test.assert_all_validators_have_decision(tx.id(), Decision::Commit)
            .await;
        test.assert_all_validators_committed(tx.id());
    }
    // One large value fits the block budget and two do not, so no block carries more than one of the transactions
    let per_block = committed_transaction_commands_per_block(&test);
    assert_eq!(per_block.iter().sum::<usize>(), NUM_TRANSACTIONS, "{per_block:?}");
    assert!(
        per_block.iter().all(|count| *count <= 1),
        "a block loaded more input bytes than the budget: {per_block:?}"
    );
    test.assert_clean_shutdown().await;
}
