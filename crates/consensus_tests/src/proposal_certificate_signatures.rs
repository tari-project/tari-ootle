//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Tests for how a proposal certificate's signer bitmap is checked against the committee
//! (`check_quorum_certificate_signatures` in `crates/consensus/src/validations/common.rs`).
//!
//! A certificate names its signers by their position in the committee, which is ascending by public key, and lists
//! one signature per signer in that order. A bitmap that does not fit the committee, or a signature list that does
//! not match it, is rejected before any signature is verified.

use tari_consensus::{check_quorum_certificate_signatures, hotstuff::ProposalValidationError};
use tari_consensus_types::{BlockId, ProposalCertificate, ToSignatureMessage, ValidatorSchnorrSignature};
use tari_crypto::{keys::SecretKey, tari_utilities::ByteArray};
use tari_ootle_common_types::{
    Epoch,
    NodeHeight,
    NumPreshards,
    ProtocolVersion,
    ShardGroup,
    VotePower,
    committee::{Committee, CommitteeMember},
};
use tari_ootle_transaction::Network;
use tari_sidechain::{
    CompressedPublicKey,
    FixedHash,
    PrivateKey,
    ProposalVoteMessage,
    QcSignatures,
    QuorumDecision,
    SignerBitmap,
    ValidatorBlockSignature,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::support::{TestAddress, TestConsensusSpec, TestVoteSignatureService, helpers::derive_keypair_from_address};

const NETWORK: Network = Network::LocalNet;
const EPOCH: Epoch = Epoch(1);
const HEIGHT: NodeHeight = NodeHeight(5);

/// Four members of one vote power each, so three signers reach quorum.
fn committee() -> Committee<TestAddress> {
    weighted_committee(&[("1", 1), ("2", 1), ("3", 1), ("4", 1)])
}

fn weighted_committee(members: &[(&str, u64)]) -> Committee<TestAddress> {
    members
        .iter()
        .map(|(name, power)| {
            let address = TestAddress::new(*name);
            let (_, public_key) = derive_keypair_from_address(&address);
            CommitteeMember {
                address,
                public_key: RistrettoPublicKeyBytes::from_bytes(public_key.as_bytes()).unwrap(),
                vote_power: VotePower::of(*power),
            }
        })
        .collect()
}

/// The committee indices of the members named `names`, ascending as a signer bitmap lists them.
fn indices_of(committee: &Committee<TestAddress>, names: &[&str]) -> Vec<usize> {
    let mut indices = names
        .iter()
        .map(|name| {
            committee
                .address_iter()
                .position(|address| *address == TestAddress::new(*name))
                .unwrap()
        })
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices
}

fn certificate(signatures: QcSignatures) -> ProposalCertificate {
    ProposalCertificate::new(
        FixedHash::from([1u8; 32]),
        BlockId::from([2u8; 32]),
        HEIGHT,
        EPOCH,
        ShardGroup::all_shards(NumPreshards::P256),
        signatures,
        QuorumDecision::Accept,
    )
}

/// The signature of the committee member at `index` over the certificate this module builds.
fn sign(committee: &Committee<TestAddress>, index: usize) -> ValidatorBlockSignature {
    let (secret, _) = derive_keypair_from_address(&committee.get(index).unwrap().address);
    let block_id = certificate(ProposalCertificate::genesis_signatures()).calculate_block_id();
    let message = ProposalVoteMessage::new(
        ProtocolVersion::at(NETWORK, EPOCH).as_u32(),
        block_id.hash(),
        QuorumDecision::Accept,
        EPOCH.as_u64(),
        HEIGHT.as_u64(),
    )
    .to_signature_message();
    let nonce = PrivateKey::from_uniform_bytes(&[index as u8 + 1; 64]).unwrap();
    let signature = ValidatorSchnorrSignature::sign_with_nonce_and_message(&secret, nonce, message).unwrap();
    ValidatorBlockSignature::new(
        CompressedPublicKey::from_canonical_bytes(signature.get_public_nonce().as_bytes()).unwrap(),
        signature.get_signature().clone(),
    )
}

fn signed_by(committee: &Committee<TestAddress>, indices: &[usize]) -> QcSignatures {
    QcSignatures::Schnorr {
        signers: SignerBitmap::from_indices(committee.len(), indices.iter().copied()).unwrap(),
        signatures: indices.iter().map(|index| sign(committee, *index)).collect(),
    }
}

fn check(committee: &Committee<TestAddress>, signatures: QcSignatures) -> Result<(), ProposalValidationError> {
    let verifier = TestVoteSignatureService::new(TestAddress::new("verifier"));
    check_quorum_certificate_signatures::<TestConsensusSpec>(
        NETWORK,
        (&certificate(signatures)).into(),
        committee,
        &verifier,
    )
}

#[test]
fn a_quorum_of_signers_verifies() {
    let committee = committee();
    check(&committee, signed_by(&committee, &[0, 2, 3])).unwrap();
}

#[test]
fn signers_short_of_quorum_are_rejected() {
    let committee = committee();
    let err = check(&committee, signed_by(&committee, &[0, 2])).unwrap_err();
    assert!(
        matches!(err, ProposalValidationError::QuorumWasNotReached { .. }),
        "{err}"
    );
}

#[test]
fn quorum_is_counted_in_vote_power_not_signers() {
    // n = 8, f = 2, quorum_threshold = 6
    let committee = weighted_committee(&[("heavy", 5), ("1", 1), ("2", 1), ("3", 1)]);
    assert_eq!(committee.quorum_threshold(), VotePower::of(6));

    for short in [&["1", "2", "3"][..], &["heavy"]] {
        let err = check(&committee, signed_by(&committee, &indices_of(&committee, short))).unwrap_err();
        assert!(
            matches!(err, ProposalValidationError::QuorumWasNotReached { .. }),
            "{short:?}: {err}"
        );
    }

    check(
        &committee,
        signed_by(&committee, &indices_of(&committee, &["heavy", "2"])),
    )
    .unwrap();
}

#[test]
fn a_bitmap_sized_for_another_committee_is_rejected() {
    let committee = committee();
    let QcSignatures::Schnorr { signatures, .. } = signed_by(&committee, &[0, 1, 2]);
    let signatures = QcSignatures::Schnorr {
        signers: SignerBitmap::from_indices(9, [0, 1, 2]).unwrap(),
        signatures,
    };
    let err = check(&committee, signatures).unwrap_err();
    assert!(
        matches!(err, ProposalValidationError::QcMalformedSignatures { .. }),
        "{err}"
    );
}

#[test]
fn a_signature_count_that_differs_from_the_signers_is_rejected() {
    let committee = committee();
    let QcSignatures::Schnorr {
        signers,
        mut signatures,
    } = signed_by(&committee, &[0, 1, 2, 3]);
    signatures.pop();
    let err = check(&committee, QcSignatures::Schnorr { signers, signatures }).unwrap_err();
    assert!(
        matches!(err, ProposalValidationError::QcMalformedSignatures { .. }),
        "{err}"
    );
}

#[test]
fn signatures_out_of_committee_order_are_rejected() {
    let committee = committee();
    let QcSignatures::Schnorr {
        signers,
        mut signatures,
    } = signed_by(&committee, &[0, 1, 2]);
    signatures.swap(0, 1);
    let err = check(&committee, QcSignatures::Schnorr { signers, signatures }).unwrap_err();
    assert!(
        matches!(err, ProposalValidationError::QcInvalidSignature { .. }),
        "{err}"
    );
}

#[test]
fn a_bitmap_naming_a_member_who_did_not_sign_is_rejected() {
    let committee = committee();
    let QcSignatures::Schnorr { signatures, .. } = signed_by(&committee, &[0, 1, 2]);
    let signatures = QcSignatures::Schnorr {
        signers: SignerBitmap::from_indices(committee.len(), [0, 1, 3]).unwrap(),
        signatures,
    };
    let err = check(&committee, signatures).unwrap_err();
    assert!(
        matches!(err, ProposalValidationError::QcInvalidSignature { .. }),
        "{err}"
    );
}
