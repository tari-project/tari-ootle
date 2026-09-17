//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for the rule that binds a proposal to the certificate height its timeout certificate attests to.
//!
//! Every timeout vote signs the height of the certificate its signer held when the view failed, so a timeout
//! certificate proves that a quorum has reached that height. The rule is implemented by
//! `check_justify_reaches_timeout_certificate` (`crates/consensus/src/validations/common.rs`): a proposal built on
//! that timeout certificate must justify from a certificate at least that high, so a leader cannot use a leader
//! failure to rewind the committee onto an older branch.

use std::collections::BTreeSet;

use tari_common_types::types::FixedHash;
use tari_consensus::{check_justify_reaches_timeout_certificate, messages::NewViewMessage};
use tari_consensus_types::{
    BlockId,
    ProposalCertificate,
    ShardGroupAccumulatedData,
    SignedTimeout,
    TimeoutCertificate,
    TimeoutVote,
    ValidatorSignatureBytes,
};
use tari_crypto::tari_utilities::epoch_time::EpochTime;
use tari_ootle_common_types::{Epoch, ExtraData, NodeHeight, NumPreshards, ProtocolVersion, ShardGroup};
use tari_ootle_storage::consensus_models::{Block, BlockHeader};
use tari_ootle_transaction::Network;
use tari_sidechain::QuorumDecision;
use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

const NUM_PRESHARDS: NumPreshards = NumPreshards::P256;
const NETWORK: Network = Network::LocalNet;
const TEST_EPOCH: Epoch = Epoch(0);

/// A timeout certificate for `height` whose signers attest to `high_pc_heights`.
fn timeout_certificate(height: NodeHeight, high_pc_heights: &[u64]) -> TimeoutCertificate {
    TimeoutCertificate::new(
        TEST_EPOCH,
        height,
        high_pc_heights
            .iter()
            .map(|h| SignedTimeout {
                high_pc_height: NodeHeight(*h),
                signature: ValidatorSignatureBytes::new(
                    RistrettoPublicKeyBytes::default(),
                    SchnorrSignatureBytes::zero(),
                ),
            })
            .collect(),
    )
}

fn proposal_certificate(height: NodeHeight) -> ProposalCertificate {
    ProposalCertificate::new(
        FixedHash::zero(),
        BlockId::zero(),
        height,
        TEST_EPOCH,
        ShardGroup::all_shards(NUM_PRESHARDS),
        vec![],
        QuorumDecision::Accept,
    )
}

fn timeout_vote(height: NodeHeight, high_pc_height: NodeHeight) -> TimeoutVote {
    TimeoutVote {
        epoch: TEST_EPOCH,
        height,
        high_pc_height,
        signature: ValidatorSignatureBytes::new(RistrettoPublicKeyBytes::default(), SchnorrSignatureBytes::zero()),
    }
}

fn block_justifying(justify_height: NodeHeight, timeout_certificate: TimeoutCertificate) -> Block {
    let justify = proposal_certificate(justify_height);
    let height = timeout_certificate.height() + NodeHeight(1);
    let header = BlockHeader::create_unsigned(
        NETWORK,
        ProtocolVersion::at(NETWORK, TEST_EPOCH),
        BlockId::zero(),
        justify.calculate_id(),
        height,
        TEST_EPOCH,
        ShardGroup::all_shards(NUM_PRESHARDS),
        RistrettoPublicKeyBytes::default(),
        FixedHash::zero(),
        &BTreeSet::new(),
        0,
        EpochTime::now().as_u64(),
        FixedHash::zero(),
        ShardGroupAccumulatedData::default(),
        ExtraData::new(),
    )
    .unwrap();
    Block::new(header, justify, BTreeSet::new(), Some(timeout_certificate))
}

#[test]
fn justify_at_the_attested_height_is_accepted() {
    let tc = timeout_certificate(NodeHeight(9), &[3, 5, 4]);
    let block = block_justifying(NodeHeight(5), tc);

    check_justify_reaches_timeout_certificate(&block).unwrap();
}

/// One signer attesting to a higher certificate is enough: that certificate exists, so the branch below it is
/// settled and a proposal may not justify from under it.
#[test]
fn justify_below_the_attested_height_is_rejected() {
    let tc = timeout_certificate(NodeHeight(9), &[3, 5, 4]);
    let block = block_justifying(NodeHeight(4), tc);

    let err = check_justify_reaches_timeout_certificate(&block).unwrap_err();
    assert!(
        matches!(
            err,
            tari_consensus::hotstuff::ProposalValidationError::JustifyBelowTimeoutCertificate { .. }
        ),
        "unexpected error: {err}"
    );
}

/// A block with no timeout certificate has nothing to reach.
#[test]
fn a_block_without_a_timeout_certificate_is_accepted() {
    let tc = timeout_certificate(NodeHeight(9), &[3, 5, 4]);
    let mut block = block_justifying(NodeHeight(1), tc);
    block = Block::new(block.header().clone(), block.justify().clone(), BTreeSet::new(), None);

    check_justify_reaches_timeout_certificate(&block).unwrap();
}

/// The claim a timeout vote signs is only worth the certificate carried beside it: every replica other than the
/// one that receives this message takes the signature on trust, so a claim above what the sender can show would
/// bind the committee to a height no leader can reach.
#[test]
fn a_timeout_claim_above_the_carried_certificate_is_not_matched() {
    let high_pc = proposal_certificate(NodeHeight(5));
    let message = NewViewMessage {
        high_pc: high_pc.clone(),
        last_vote: None,
        timeout: timeout_vote(NodeHeight(9), NodeHeight(6)),
    };
    assert!(!message.timeout_claim_matches_high_pc());

    let message = NewViewMessage {
        high_pc,
        last_vote: None,
        timeout: timeout_vote(NodeHeight(9), NodeHeight(5)),
    };
    assert!(message.timeout_claim_matches_high_pc());
}
