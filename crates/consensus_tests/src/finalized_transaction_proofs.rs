//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_consensus_types::Decision;
use tari_engine_types::commit_result::AbortReason;
use tari_ootle_common_types::{Epoch, NodeHeight, optional::Optional};
use tari_ootle_storage::{
    EpochCleanupStep,
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    StorageError,
    consensus_models::{Block, CommittedBlockProof, FinalizedTransactionLeaf},
};
use tari_ootle_transaction::TransactionId;
use tari_state_tree::{JmtHashScheme, TreeHash};

use crate::support::{Test, TestAddress, TestVnDestination, logging::setup_logger};

/// The decision a validator finalizes for a transaction stays provable against the header of the block that finalized
/// it, from the inclusion and commit proof stored when the block committed, after the block itself is pruned.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_finalized_decision_is_provable_after_its_block_is_pruned() {
    let mut test = start_test().await;
    let (commit, _, _) = test.send_transaction_to_all(Decision::Commit, 100, 1, 1).await;
    let (abort, _, _) = test.send_transaction_to_all(ABORT, 1, 1, 1).await;
    finalize_and_assert_provable(test, &[(*commit.id(), Decision::Commit), (*abort.id(), ABORT)]).await;
}

/// A block that only aborts transactions produces no state versions, and its commit proof is stored for the aborts
/// alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_abort_is_provable_when_its_block_changes_no_state() {
    let mut test = start_test().await;
    let (abort, _, _) = test.send_transaction_to_all(ABORT, 1, 1, 1).await;
    finalize_and_assert_provable(test, &[(*abort.id(), ABORT)]).await;
}

const ABORT: Decision = Decision::Abort(AbortReason::ExecutionFailure);

async fn start_test() -> Test {
    setup_logger();
    Test::builder()
        .modify_config(|config_mut| {
            config_mut.epoch_end_grace_period = Duration::from_secs(0);
        })
        .modify_consensus_constants(|config| {
            config.pacemaker_block_time = Duration::from_millis(500);
        })
        .add_committee(0, vec!["1"])
        .start()
        .await
}

/// Runs epoch 1 until `transactions` are finalized, prunes their blocks, and checks that each finalized the expected
/// decision and that the decision verifies against the stored commit proof of its block.
async fn finalize_and_assert_provable(mut test: Test, transactions: &[(TransactionId, Decision)]) {
    test.start_epoch(Epoch(1)).await;
    loop {
        test.on_block_committed().await;
        if transactions
            .iter()
            .all(|(id, _)| test.is_transaction_finalized_at_destination(TestVnDestination::All, id))
        {
            break;
        }
        let leaf = test.get_validator(&TestAddress::new("1")).get_leaf_block();
        if leaf.height >= NodeHeight(20) {
            panic!("Not all transactions finalized after {} blocks", leaf.height);
        }
    }
    test.stop();

    let store = &test.get_validator(&TestAddress::new("1")).state_store;
    // Prunes the blocks of every epoch before 100 and keeps the transaction bookkeeping.
    store
        .with_write_tx(|tx| tx.epoch_cleanup_step(Epoch(100), EpochCleanupStep::Blocks, usize::MAX))
        .unwrap();

    store
        .with_read_tx(|tx| {
            for (transaction_id, expected) in transactions {
                let decision = tx.finalized_transaction_execution_get(transaction_id)?.decision();
                assert_eq!(decision, *expected, "{transaction_id} finalized the wrong decision");

                let inclusion = tx
                    .finalized_transaction_inclusion_get(transaction_id)?
                    .unwrap_or_else(|| panic!("{transaction_id} was finalized without an inclusion"));
                assert!(
                    Block::get(tx, &inclusion.block_id).optional()?.is_none(),
                    "block {} was not pruned, so the test does not exercise a pruned block",
                    inclusion.block_id
                );

                let commit_proof = tx
                    .block_commit_proofs_get(&inclusion.block_id)
                    .optional()?
                    .unwrap_or_else(|| panic!("no commit proof is stored for block {}", inclusion.block_id));
                let commit_proof =
                    CommittedBlockProof::from_bytes(&commit_proof).expect("the stored commit proof decodes");
                assert_eq!(commit_proof.block_id(), *inclusion.block_id.hash());
                let root = TreeHash::from(commit_proof.proof().header.transaction_merkle_root.into_array());
                let leaf = FinalizedTransactionLeaf {
                    transaction_id,
                    decision,
                };
                inclusion
                    .proof
                    .verify_inclusion(JmtHashScheme::V1, &root, &leaf.key(), &leaf.hash())
                    .unwrap_or_else(|e| panic!("{transaction_id}'s decision does not verify: {e}"));
            }
            Ok::<_, StorageError>(())
        })
        .unwrap();
}
