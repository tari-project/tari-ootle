//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for the rule that a proposal's justify accepts a lower block of the proposal's own shard group
//! (`check_justify_precedes_block` in `crates/consensus/src/validations/common.rs`).
//!
//! The shard group a certificate names lies outside both its id and the vote message its signers sign, so a
//! certificate relabelled to another shard group keeps its id and its signatures. The proposal is what binds it.

use tari_consensus::{check_justify_precedes_block, hotstuff::ProposalValidationError};
use tari_consensus_types::{BlockId, ProposalCertificate, ShardGroupAccumulatedData};
use tari_crypto::tari_utilities::epoch_time::EpochTime;
use tari_ootle_common_types::{Epoch, ExtraData, NodeHeight, NumPreshards, ProtocolVersion, ShardGroup};
use tari_ootle_storage::consensus_models::{Block, BlockCommands, BlockHeader};
use tari_ootle_transaction::Network;
use tari_sidechain::{FixedHash, QuorumDecision};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

const NETWORK: Network = Network::LocalNet;
const TEST_EPOCH: Epoch = Epoch(1);
const BLOCK_HEIGHT: NodeHeight = NodeHeight(6);
const JUSTIFY_HEIGHT: NodeHeight = NodeHeight(5);

fn block_shard_group() -> ShardGroup {
    ShardGroup::new(0, 127)
}

fn justify(height: NodeHeight, shard_group: ShardGroup, decision: QuorumDecision) -> ProposalCertificate {
    ProposalCertificate::new(
        FixedHash::from([1u8; 32]),
        BlockId::from([2u8; 32]),
        height,
        TEST_EPOCH,
        shard_group,
        ProposalCertificate::genesis_signatures(),
        decision,
    )
}

/// A block of `block_shard_group()` at `BLOCK_HEIGHT` justified by `justify`.
fn block_justified_by(justify: ProposalCertificate) -> Block {
    let header = BlockHeader::create_unsigned(
        NETWORK,
        ProtocolVersion::at(NETWORK, TEST_EPOCH),
        justify.calculate_block_id(),
        justify.calculate_id(),
        None,
        BLOCK_HEIGHT,
        TEST_EPOCH,
        block_shard_group(),
        RistrettoPublicKeyBytes::default(),
        FixedHash::zero(),
        &BlockCommands::empty(),
        0,
        EpochTime::now().as_u64(),
        FixedHash::zero(),
        ShardGroupAccumulatedData::default(),
        ExtraData::new(),
    )
    .unwrap();
    Block::new(header, justify, BlockCommands::empty(), None)
}

#[test]
fn an_accept_justify_of_a_lower_block_in_the_same_shard_group_is_accepted() {
    let block = block_justified_by(justify(JUSTIFY_HEIGHT, block_shard_group(), QuorumDecision::Accept));
    check_justify_precedes_block(&block).unwrap();
}

#[test]
fn a_justify_for_another_shard_group_is_rejected() {
    let relabelled = justify(
        JUSTIFY_HEIGHT,
        ShardGroup::all_shards(NumPreshards::P256),
        QuorumDecision::Accept,
    );
    let genuine = justify(JUSTIFY_HEIGHT, block_shard_group(), QuorumDecision::Accept);
    assert_ne!(relabelled.shard_group(), genuine.shard_group());
    assert_eq!(relabelled.calculate_id(), genuine.calculate_id());

    let err = check_justify_precedes_block(&block_justified_by(relabelled)).unwrap_err();
    assert!(
        matches!(err, ProposalValidationError::InvalidShardGroup { .. }),
        "{err}"
    );
}

#[test]
fn a_reject_justify_is_rejected() {
    let block = block_justified_by(justify(JUSTIFY_HEIGHT, block_shard_group(), QuorumDecision::Reject));
    let err = check_justify_precedes_block(&block).unwrap_err();
    assert!(
        matches!(err, ProposalValidationError::JustifyBlockInvalid { .. }),
        "{err}"
    );
}

#[test]
fn a_justify_at_or_above_the_block_is_rejected() {
    let block = block_justified_by(justify(BLOCK_HEIGHT, block_shard_group(), QuorumDecision::Accept));
    let err = check_justify_precedes_block(&block).unwrap_err();
    assert!(
        matches!(err, ProposalValidationError::CandidateBlockNotHigherThanJustify { .. }),
        "{err}"
    );
}
