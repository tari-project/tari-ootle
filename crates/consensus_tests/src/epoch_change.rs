//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_consensus::hotstuff::HotStuffError;
use tari_consensus_types::Decision;
use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::{StateStore, StateStoreReadTransaction};

use crate::support::{Test, logging::setup_logger};

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
