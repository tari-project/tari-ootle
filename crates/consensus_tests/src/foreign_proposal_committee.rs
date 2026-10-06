//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for which committee a foreign proposal's proof is verified against.

use std::collections::HashMap;

use ootle_byte_type::ToByteType;
use tari_common_types::types::CompressedPublicKey;
use tari_consensus::{
    hotstuff::{HotStuffError, ProposalValidationError},
    resolve_foreign_committee,
};
use tari_ootle_common_types::{
    ShardGroup,
    VotePower,
    committee::{Committee, CommitteeMember},
};
use tari_ootle_storage::consensus_models::{BlockPledge, CommandsCommitProof, ForeignProposal};
use tari_ootle_transaction::Network;
use tari_sidechain::{SidechainBlockCommitProof, SidechainBlockHeader};
use tokio::sync::broadcast;

use crate::support::{TEST_NUM_PRESHARDS, TestAddress, TestEpochManager, committee_number_to_shard_group, helpers};

fn committee(addresses: &[&'static str]) -> Committee<TestAddress> {
    Committee::new(
        addresses
            .iter()
            .map(|addr| {
                let address = TestAddress::new(*addr);
                let (_, public_key) = helpers::derive_keypair_from_address(&address);
                CommitteeMember {
                    address,
                    public_key: public_key.to_byte_type(),
                    vote_power: VotePower::of(1),
                }
            })
            .collect(),
    )
}

/// A proposal whose header names `shard_group` and whose proposer is `proposed_by`.
fn proposal(proposed_by: &'static str, shard_group: tari_sidechain::ShardGroup) -> ForeignProposal {
    let (_, proposer) = helpers::derive_keypair_from_address(&TestAddress::new(proposed_by));
    let commit_proof = CommandsCommitProof::new_latest(vec![], SidechainBlockCommitProof {
        header: SidechainBlockHeader {
            network: Network::LocalNet.as_byte(),
            protocol_version: 0,
            parent_id: Default::default(),
            justify_id: Default::default(),
            height: 1,
            epoch: 1,
            epoch_hash: Default::default(),
            shard_group,
            proposed_by: CompressedPublicKey::new_from_pk(proposer),
            state_merkle_root: Default::default(),
            command_merkle_root: Default::default(),
            transaction_merkle_root: None,
            signature: Default::default(),
            accumulated_data: Default::default(),
            metadata_hash: Default::default(),
        },
        proof_elements: vec![],
    });
    ForeignProposal::new(commit_proof, BlockPledge::default())
}

fn to_header_shard_group(shard_group: ShardGroup) -> tari_sidechain::ShardGroup {
    tari_sidechain::ShardGroup {
        start: shard_group.start().as_u32(),
        end_inclusive: shard_group.end().as_u32(),
    }
}

struct Setup {
    epoch_manager: TestEpochManager,
    group_b: ShardGroup,
    committee_b: Committee<TestAddress>,
}

async fn setup() -> Setup {
    let group_a = committee_number_to_shard_group(TEST_NUM_PRESHARDS, 0, 2);
    let group_b = committee_number_to_shard_group(TEST_NUM_PRESHARDS, 1, 2);
    let committee_a = committee(&["a1", "a2", "a3"]);
    let committee_b = committee(&["b1", "b2", "b3"]);

    let (tx_events, _) = broadcast::channel(1);
    let epoch_manager = TestEpochManager::new(tx_events);
    epoch_manager
        .add_committees(HashMap::from([(group_a, committee_a), (group_b, committee_b.clone())]))
        .await;

    Setup {
        epoch_manager,
        group_b,
        committee_b,
    }
}

#[tokio::test]
async fn it_verifies_against_the_named_shard_groups_committee_not_the_proposers() {
    let Setup {
        epoch_manager,
        group_b,
        committee_b,
    } = setup().await;

    // A member of shard group A proposes a block whose header names shard group B
    let proposal = proposal("a1", to_header_shard_group(group_b));

    let committee = resolve_foreign_committee(&epoch_manager, &proposal)
        .await
        .unwrap()
        .expect("shard group B has a committee");
    assert_eq!(*committee, committee_b);
    assert!(!committee.contains_public_key(&proposal.proposed_by()));
}

#[tokio::test]
async fn it_finds_no_committee_for_a_shard_group_that_is_not_assigned() {
    let Setup {
        epoch_manager, group_b, ..
    } = setup().await;

    // A sub-range of shard group B has valid bounds but no committee of its own
    let sub_range = tari_sidechain::ShardGroup {
        start: group_b.start().as_u32(),
        end_inclusive: group_b.start().as_u32() + 1,
    };
    let proposal = proposal("b1", sub_range);

    let committee = resolve_foreign_committee(&epoch_manager, &proposal).await.unwrap();
    assert!(committee.is_none());
}

#[tokio::test]
async fn it_rejects_a_shard_group_with_invalid_bounds() {
    let Setup { epoch_manager, .. } = setup().await;

    let proposal = proposal("b1", tari_sidechain::ShardGroup {
        start: 10,
        end_inclusive: 5,
    });

    let err = resolve_foreign_committee(&epoch_manager, &proposal).await.unwrap_err();
    assert!(
        matches!(
            err,
            HotStuffError::ProposalValidationError(ProposalValidationError::InvalidShardGroup { .. })
        ),
        "unexpected error: {err}"
    );
}
