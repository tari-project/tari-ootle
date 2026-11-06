//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for a local proposal delivered without the foreign proposals its block carries.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tari_common_types::types::PrivateKey;
use tari_consensus::messages::{HotstuffMessage, ProposalMessage};
use tari_consensus_types::Decision;
use tari_ootle_common_types::{Epoch, NodeHeight, SubstateLockType};
use tari_ootle_storage::{StateStore, StateStoreReadTransaction, StorageError, consensus_models::Block};
use tari_ootle_transaction::Transaction;

use crate::support::{Test, TestAddress, TestVnDestination, build_transaction_from, logging::setup_logger};

/// A replica that has not received a foreign proposal its block carries does not keep the block, so a later delivery
/// of the block with the foreign proposal is processed rather than discarded as already seen.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_block_delivered_without_its_foreign_proposals_is_processed_on_a_later_delivery() {
    setup_logger();
    let victim = TestAddress::new("4");
    // The first proposal to `victim` that carries foreign proposals, and the member that sent it. Once captured,
    // nothing more reaches `victim`, so its view stays where that proposal extends it.
    let captured = Arc::new(Mutex::new(None::<(TestAddress, ProposalMessage)>));
    let mut test = Test::builder()
        .add_committee(0, vec!["1", "2", "3", "4"])
        .add_committee(1, vec!["5"])
        .with_test_timeout(Duration::from_secs(60))
        .with_message_filter(Box::new({
            let victim = victim.clone();
            let captured = captured.clone();
            move |from, to, msg| {
                if *to != victim {
                    return true;
                }
                let mut captured = captured.lock().unwrap();
                if captured.is_some() {
                    return false;
                }
                match msg {
                    HotstuffMessage::ForeignProposal(_) | HotstuffMessage::ForeignProposalNotification(_) => false,
                    HotstuffMessage::CatchUpSyncResponse(proposal) => proposal.foreign_proposals.is_empty(),
                    HotstuffMessage::Proposal(proposal) if !proposal.foreign_proposals.is_empty() => {
                        *captured = Some((from.clone(), (**proposal).clone()));
                        false
                    },
                    _ => true,
                }
            }
        }))
        .start()
        .await;
    let inputs = test.create_substates_on_vns(TestVnDestination::Committee(0), 1);
    let outputs = test.build_outputs_for_committee(1, 1);
    let transaction = build_transaction_from(
        Transaction::builder_localnet(Epoch(1))
            .with_inputs(inputs.iter().cloned().map(Into::into))
            .build_and_seal(&PrivateKey::default()),
    );
    test.create_execution_at_destination_for_transaction(
        TestVnDestination::All,
        &transaction,
        Decision::Commit,
        5,
        inputs
            .into_iter()
            .map(|input| (input.substate_id().clone(), SubstateLockType::Write))
            .collect(),
        outputs,
    );
    test.send_transaction_to_destination(TestVnDestination::All, transaction)
        .await;
    test.start_epoch(Epoch(1)).await;

    let (sender, proposal) = loop {
        let (_, _, _, height) = test.on_block_committed().await;
        if let Some(captured) = captured.lock().unwrap().clone() {
            break captured;
        }
        assert!(height < NodeHeight(30), "no proposal carried a foreign proposal");
    };

    let tx_inbound = test.get_validator(&victim).tx_inbound_message.clone();
    let without_foreign_proposals = ProposalMessage {
        block: proposal.block.clone(),
        foreign_proposals: vec![],
    };
    for message in [without_foreign_proposals, proposal.clone()] {
        tx_inbound
            .send((sender.clone(), HotstuffMessage::new_proposal(message)))
            .await
            .unwrap();
    }

    let block_id = *proposal.block.id();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let is_processed = test
            .get_validator(&victim)
            .state_store
            .with_read_tx(|tx| {
                if !Block::record_exists(tx, &block_id)? {
                    return Ok::<_, StorageError>(false);
                }
                for foreign_proposal in &proposal.foreign_proposals {
                    if !tx.foreign_proposals_exists(&foreign_proposal.calculate_block_id())? {
                        return Ok(false);
                    }
                }
                Ok(true)
            })
            .unwrap();
        if is_processed {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{victim} did not store block {block_id} with its foreign proposals"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    test.stop();
}
