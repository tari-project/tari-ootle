//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for how a proposer treats a stored foreign proposal that fails to process.

use std::time::Duration;

use tari_ootle_common_types::{Epoch, NodeHeight};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StorageError,
    consensus_models::{
        BlockHeaderMetadata,
        BlockPledge,
        CommandsCommitProof,
        ForeignProposal,
        ForeignProposalRecord,
        ForeignProposalStatus,
    },
};
use tari_ootle_transaction::Network;
use tari_sidechain::{SidechainBlockCommitProof, SidechainBlockHeader};

use crate::support::{Test, logging::setup_logger};

/// A foreign proposal whose protocol version cannot be decoded by the processor.
fn foreign_proposal_with_unsupported_protocol_version() -> ForeignProposalRecord {
    let metadata = BlockHeaderMetadata {
        total_leader_fee: 0,
        timestamp: 0,
        extra_data: Default::default(),
        timeout_certificate_id: None,
        is_dummy: false,
    };
    let header = SidechainBlockHeader {
        network: Network::LocalNet.as_byte(),
        protocol_version: 255,
        parent_id: Default::default(),
        justify_id: Default::default(),
        height: 1,
        epoch: 1,
        epoch_hash: Default::default(),
        shard_group: tari_sidechain::ShardGroup {
            start: 0,
            end_inclusive: 127,
        },
        proposed_by: Default::default(),
        state_merkle_root: Default::default(),
        command_merkle_root: Default::default(),
        transaction_merkle_root: Default::default(),
        signature: Default::default(),
        accumulated_data: Default::default(),
        metadata_hash: metadata.calculate_hash(),
    };
    let proof = CommandsCommitProof::new_latest(vec![], SidechainBlockCommitProof {
        header,
        proof_elements: vec![],
    });
    ForeignProposalRecord::new(ForeignProposal::new(proof, BlockPledge::default(), metadata))
}

/// With no transactions to propose, a leader still processes the foreign proposals it selects. One that fails
/// validation is marked invalid and left out, so the committee keeps committing blocks rather than refusing every
/// block that carries it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_invalid_foreign_proposal_is_dropped_from_a_block_without_transactions() {
    setup_logger();
    let mut test = Test::builder()
        .add_committee(0, vec!["1", "2", "3", "4"])
        .with_test_timeout(Duration::from_secs(60))
        .start()
        .await;
    test.start_epoch(Epoch(1)).await;
    test.wait_for_all_validators_to_start_consensus().await;
    let (_, _, _, seeded_at) = test.on_block_committed().await;

    let foreign_proposal = foreign_proposal_with_unsupported_protocol_version();
    test.with_all_validators(|validator| {
        validator
            .state_store
            .with_write_tx(|tx| foreign_proposal.save(tx))
            .unwrap();
    });

    loop {
        let (_, _, _, height) = test.on_block_committed().await;
        if height >= seeded_at + NodeHeight(3) {
            break;
        }
    }
    test.stop();

    let statuses = test
        .validators_iter()
        .map(|validator| {
            validator
                .state_store
                .with_read_tx(|tx| {
                    let [record] = tx
                        .foreign_proposals_get_any([foreign_proposal.block_id()])?
                        .try_into()
                        .unwrap();
                    Ok::<_, StorageError>(record.status())
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        statuses.contains(&ForeignProposalStatus::Invalid),
        "no leader marked the foreign proposal invalid: {statuses:?}"
    );
    assert!(
        !statuses.contains(&ForeignProposalStatus::Confirmed),
        "a committed block carries the invalid foreign proposal: {statuses:?}"
    );

    test.assert_clean_shutdown().await;
}
