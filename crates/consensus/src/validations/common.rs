//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use std::collections::HashSet;

use log::{debug, warn};
use tari_common_types::types::FixedHash;
use tari_consensus_types::{QuorumCertificateRef, TimeoutVote, ValidatorSignatureBytes};
use tari_ootle_common_types::{
    DerivableFromPublicKey,
    Epoch,
    ExtraFieldKey,
    NodeHeight,
    NumPreshards,
    ProtocolVersion,
    ShardGroup,
    VotePower,
    committee::Committee,
};
use tari_ootle_storage::consensus_models::{Block, BlockHeader};
use tari_ootle_transaction::Network;
use tari_sidechain::ProposalVoteMessage;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    hotstuff::{HotstuffConfig, ProposalValidationError},
    traits::{ConsensusSpec, LeaderStrategy, ValidatorSignatureVerifierService},
    validations::signed_vote::SignedProposalVote,
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::validations";

pub(super) fn check_current_epoch(
    candidate_block: &Block,
    current_epoch: Epoch,
) -> Result<(), ProposalValidationError> {
    if candidate_block.epoch() > current_epoch {
        warn!(target: LOG_TARGET, "⚠️ Proposal for future epoch {} received. Current epoch is {}", candidate_block.epoch(), current_epoch);
        return Err(ProposalValidationError::FutureEpoch {
            block_id: *candidate_block.id(),
            current_epoch,
            block_epoch: candidate_block.epoch(),
        });
    }

    Ok(())
}

pub(super) fn check_network(header: &BlockHeader, network: Network) -> Result<(), ProposalValidationError> {
    if header.network() != network {
        return Err(ProposalValidationError::InvalidNetwork {
            block_network: header.network().to_string(),
            expected_network: network.to_string(),
            block_id: *header.id(),
        });
    }
    Ok(())
}

/// The protocol version a block is produced under is fixed by the network's activation schedule at the block's
/// epoch. A node whose schedule disagrees rejects the block instead of hashing it under a schema the rest of the
/// network has left behind, which stalls the node rather than forking it.
pub(super) fn check_protocol_version(header: &BlockHeader, network: Network) -> Result<(), ProposalValidationError> {
    let expected_version = ProtocolVersion::at(network, header.epoch());
    if header.protocol_version() != expected_version {
        return Err(ProposalValidationError::InvalidProtocolVersion {
            expected_version,
            block_version: header.protocol_version(),
            epoch: header.epoch(),
            block_id: *header.id(),
        });
    }
    Ok(())
}

pub(super) fn check_epoch_hash(
    header: &BlockHeader,
    expected_epoch_hash: &FixedHash,
) -> Result<(), ProposalValidationError> {
    if header.epoch_hash() != expected_epoch_hash {
        return Err(ProposalValidationError::InvalidEpochHash {
            epoch: header.epoch(),
            local_epoch_hash: *expected_epoch_hash,
            invalid_epoch_hash: *header.epoch_hash(),
            block_id: *header.id(),
        });
    }

    Ok(())
}

pub(super) fn check_shard_group_matches(
    header: &BlockHeader,
    expected_shard_group: ShardGroup,
) -> Result<(), ProposalValidationError> {
    if header.shard_group() != expected_shard_group {
        return Err(ProposalValidationError::InvalidShardGroup {
            block_id: *header.id(),
            shard_group: header.shard_group(),
            details: format!(
                "Expected shard group {} but got {}",
                expected_shard_group,
                header.shard_group()
            ),
        });
    }

    Ok(())
}

pub(super) fn check_shard_group_bounds(
    header: &BlockHeader,
    num_preshards: NumPreshards,
) -> Result<(), ProposalValidationError> {
    let len = header
        .shard_group()
        .checked_len()
        .ok_or_else(|| ProposalValidationError::InvalidShardGroup {
            block_id: *header.id(),
            shard_group: header.shard_group(),
            details: "Shard group bounds are invalid".to_string(),
        })?;

    if header.shard_group().start().as_u32() as usize > num_preshards.num_shards() ||
        header.shard_group().end().as_u32() as usize > num_preshards.num_shards()
    {
        return Err(ProposalValidationError::InvalidShardGroup {
            block_id: *header.id(),
            shard_group: header.shard_group(),
            details: format!(
                "Shard group {} is out of bounds for {} preshards",
                header.shard_group(),
                num_preshards.num_shards()
            ),
        });
    }

    if len > num_preshards.num_shards() {
        return Err(ProposalValidationError::InvalidShardGroup {
            block_id: *header.id(),
            shard_group: header.shard_group(),
            details: format!(
                "Shard group {} is larger than the number of preshards {}",
                header.shard_group(),
                num_preshards.num_shards()
            ),
        });
    }

    Ok(())
}

pub(super) fn check_height(block: &Block) -> Result<(), ProposalValidationError> {
    if block.height().is_zero() {
        return Err(ProposalValidationError::InvalidBlockHeight {
            block_id: *block.id(),
            block_height: block.height(),
            details: "Block height is zero".to_string(),
        });
    }
    let max_certificate_height = block.max_certificate_height();
    // invariant: the block may only advance the view by 1 higher than the justified height
    if max_certificate_height.checked_add(NodeHeight(1)) != Some(block.height()) {
        return Err(ProposalValidationError::InvalidBlockHeight {
            block_id: *block.id(),
            block_height: block.height(),
            details: format!("Expected it to be one higher than the max certificate height {max_certificate_height}"),
        });
    }
    Ok(())
}

pub(super) fn check_proposed_by_leader<TAddr: DerivableFromPublicKey, TLeaderStrategy: LeaderStrategy<TAddr>>(
    leader_strategy: &TLeaderStrategy,
    local_committee: &Committee<TAddr>,
    block: &Block,
) -> Result<(), ProposalValidationError> {
    let parent_height =
        block
            .height()
            .checked_sub(NodeHeight(1))
            .ok_or_else(|| ProposalValidationError::InvalidBlockHeight {
                block_id: *block.id(),
                block_height: block.height(),
                details: "Block height is zero".to_string(),
            })?;
    let (addr, leader) = leader_strategy.get_leader(local_committee, parent_height);
    if leader != block.proposed_by() {
        return Err(ProposalValidationError::NotLeader {
            proposed_by: block.proposed_by().to_string(),
            expected_leader: format!("{} / {}", leader, addr),
            block: block.as_leaf(),
            max_certificate_height: block.max_certificate_height(),
        });
    }
    Ok(())
}

pub(super) fn check_block_signature<TSignerService: ValidatorSignatureVerifierService>(
    header: &BlockHeader,
    signer_service: &TSignerService,
) -> Result<(), ProposalValidationError> {
    if header.is_genesis() {
        // Genesis block doesn't have signatures
        return Ok(());
    }

    let validator_signature = header.signature().ok_or(ProposalValidationError::MissingSignature {
        block_id: *header.id(),
        height: header.height(),
    })?;

    debug!(
        target: LOG_TARGET,
        "Validating signature block_id={}, P={}, R={}",
        header.id(),
        header.proposed_by(),
        validator_signature.public_nonce(),
    );

    if !signer_service.verify(header) {
        return Err(ProposalValidationError::InvalidSignature {
            block_id: *header.id(),
            height: header.height(),
        });
    }
    Ok(())
}

pub(super) fn check_proposal_certificate<TConsensusSpec: ConsensusSpec>(
    network: Network,
    candidate_block: &Block,
    committee: &Committee<TConsensusSpec::Addr>,
    signing_service: &TConsensusSpec::SignerService,
) -> Result<(), ProposalValidationError> {
    let qc = candidate_block.justify();
    if candidate_block.height() <= qc.height() {
        return Err(ProposalValidationError::CandidateBlockNotHigherThanJustify {
            justify_block_height: qc.height(),
            candidate_block_height: candidate_block.height(),
        });
    }

    check_quorum_certificate_signatures::<TConsensusSpec>(network, qc.into(), committee, signing_service)?;

    Ok(())
}

pub(super) fn check_timeout_certificate<TConsensusSpec: ConsensusSpec>(
    network: Network,
    candidate_block: &Block,
    committee: &Committee<TConsensusSpec::Addr>,
    signing_service: &TConsensusSpec::SignerService,
) -> Result<(), ProposalValidationError> {
    check_block_commits_to_timeout_certificate(candidate_block)?;
    let Some(tc) = candidate_block.timeout_certificate() else {
        return Ok(());
    };
    if candidate_block.height() <= tc.height() {
        return Err(ProposalValidationError::CandidateBlockNotHigherThanJustify {
            justify_block_height: tc.height(),
            candidate_block_height: candidate_block.height(),
        });
    }

    check_quorum_certificate_signatures::<TConsensusSpec>(network, tc.into(), committee, signing_service)?;

    check_justify_reaches_timeout_certificate(candidate_block)?;

    Ok(())
}

/// Checks that the header's timeout certificate id names the certificate the block carries (or that both are
/// absent).
///
/// What makes a rule that reads the certificate read signed data is the block signature: on decode the header's
/// id is derived from the certificate the block carries (`try_convert_proto_block_header`, as for `justify_id`),
/// so a certificate swapped in flight moves the block id out from under the proposer's signature. A block that
/// arrived over the wire therefore cannot fail here; this guards a locally built block, and it holds the
/// derivation in place should the id ever be sent on the wire instead.
pub fn check_block_commits_to_timeout_certificate(block: &Block) -> Result<(), ProposalValidationError> {
    let header_tc_id = block.header().timeout_certificate_id().copied();
    let tc_id = block.timeout_certificate().map(|tc| tc.calculate_id());
    if header_tc_id != tc_id {
        return Err(ProposalValidationError::TimeoutCertificateIdMismatch {
            block_id: *block.id(),
            header_tc_id,
            tc_id,
        });
    }
    Ok(())
}

/// Checks that a block justifies from a certificate at least as high as the highest one its timeout certificate
/// attests to.
///
/// The attested heights carry weight only once the timeout certificate's signatures have been verified against the
/// committee, which `check_timeout_certificate` does before calling this. On a verified certificate a quorum signs
/// the height of the certificate it held when it timed out, so the committee has provably reached that height, and
/// a proposal justifying from lower discards blocks that a quorum has certified - which is how a leader orphans a
/// branch it does not like.
pub fn check_justify_reaches_timeout_certificate(block: &Block) -> Result<(), ProposalValidationError> {
    let Some(tc) = block.timeout_certificate() else {
        return Ok(());
    };
    let max_high_pc_height = tc.max_high_pc_height();
    if block.justify().height() < max_high_pc_height {
        return Err(ProposalValidationError::JustifyBelowTimeoutCertificate {
            block_id: *block.id(),
            justify_height: block.justify().height(),
            max_high_pc_height,
        });
    }
    Ok(())
}

/// Validates the signatures of the quorum certificate.
// pub because used in on receive NEWVIEW
pub fn check_quorum_certificate_signatures<TConsensusSpec: ConsensusSpec>(
    network: Network,
    qc: QuorumCertificateRef<'_>,
    committee: &Committee<TConsensusSpec::Addr>,
    signing_service: &TConsensusSpec::SignerService,
) -> Result<(), ProposalValidationError> {
    // The "zero block" is the deterministic per-epoch genesis, known to every node without a vote, so a
    // QC that justifies it is exempt from quorum/signature validation. Only a `ProposalCertificate` in
    // canonical genesis shape (height zero, zero parent, zero header hash) qualifies for the exemption;
    // everything else - including a `TimeoutCertificate` - falls through to the checks below.
    if let Some(pc) = qc.as_proposal_certificate() &&
        pc.height().is_zero() &&
        pc.signatures().is_empty() &&
        pc.justifies_zero_block() &&
        pc.parent_id().is_zero()
    {
        return Ok(());
    }

    let mut check_dups = HashSet::with_capacity(qc.num_signatures());
    let mut total_vote_power = VotePower::zero();
    let mut account_for = |signature: &ValidatorSignatureBytes| -> Result<(), ProposalValidationError> {
        let Some(power) = committee.get_power_by_public_key(signature.public_key()) else {
            return Err(ProposalValidationError::ValidatorNotInCommittee {
                validator: signature.public_key().to_string(),
                details: format!(
                    "QC {} signed with validator {} that is not in committee",
                    qc,
                    signature.public_key(),
                ),
            });
        };
        total_vote_power += power;
        if !check_dups.insert(*signature.public_key()) {
            return Err(ProposalValidationError::QcDuplicateSignature {
                qc: qc.calculate_id(),
                validator: *signature.public_key(),
            });
        }
        Ok(())
    };

    match qc {
        QuorumCertificateRef::ProposalCertificate(pc) => {
            let block_id = pc.calculate_block_id();
            for signature in pc.signatures() {
                account_for(signature)?;
                // `check_protocol_version` pins a block's version to `at(network, header.epoch())` before any vote
                // is cast on it, so resolving from the schedule here yields the version the signers used.
                let message = ProposalVoteMessage::new(
                    ProtocolVersion::at(network, pc.epoch()).as_u32(),
                    block_id.hash(),
                    pc.decision(),
                    pc.epoch().as_u64(),
                    pc.height().as_u64(),
                );
                let vote = SignedProposalVote { message, signature };
                if !signing_service.verify(&vote) {
                    return Err(ProposalValidationError::QcInvalidSignature { qc: qc.calculate_id() });
                }
            }
        },
        QuorumCertificateRef::TimeoutCertificate(tc) => {
            for timeout in tc.timeouts() {
                account_for(&timeout.signature)?;
                // Each signer attests to its own high certificate height, so the height it reports is part of what
                // it signed and a quorum cannot be assembled that hides one.
                let vote = TimeoutVote {
                    epoch: tc.epoch(),
                    height: tc.height(),
                    high_pc_height: timeout.high_pc_height,
                    signature: timeout.signature.clone(),
                };
                if !signing_service.verify(&vote) {
                    return Err(ProposalValidationError::QcInvalidSignature { qc: qc.calculate_id() });
                }
            }
        },
    }

    if total_vote_power < committee.quorum_threshold() {
        return Err(ProposalValidationError::QuorumWasNotReached {
            qc: qc.calculate_id(),
            got: total_vote_power,
            required: committee.quorum_threshold(),
        });
    }

    Ok(())
}

pub(super) fn check_sidechain_id(header: &BlockHeader, config: &HotstuffConfig) -> Result<(), ProposalValidationError> {
    // We only require the sidechain id on the genesis block
    if !header.is_genesis() {
        return Ok(());
    }

    // If we are using a sidechain id in the network, we need to check it matches the candidate block one
    let Some(expected_sidechain_id) = &config.sidechain_id else {
        return Ok(());
    };

    // Extract the sidechain id from the candidate block
    let extra_data = header.extra_data();
    let sidechain_id_bytes =
        extra_data
            .get(&ExtraFieldKey::SidechainId)
            .ok_or(ProposalValidationError::InvalidSidechainId {
                block_id: *header.id(),
                reason: "SidechainId key not present".to_owned(),
            })?;
    let sidechain_id = RistrettoPublicKeyBytes::from_bytes(sidechain_id_bytes.as_ref()).map_err(|e| {
        ProposalValidationError::InvalidSidechainId {
            block_id: *header.id(),
            reason: e.to_string(),
        }
    })?;

    // The sidechain id must match the sidechain of the current network
    if sidechain_id != *expected_sidechain_id {
        return Err(ProposalValidationError::MismatchedSidechainId {
            block_id: *header.id(),
            expected_sidechain_id: *expected_sidechain_id,
            sidechain_id,
        });
    }

    Ok(())
}
