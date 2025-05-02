//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
use std::collections::HashSet;

use log::{debug, warn};
use tari_common::configuration::Network;
use tari_common_types::types::FixedHash;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_dan_common_types::{
    committee::Committee,
    DerivableFromPublicKey,
    Epoch,
    ExtraFieldKey,
    NumPreshards,
    ShardGroup,
};
use tari_dan_storage::consensus_models::{Block, BlockHeader, QuorumCertificate, ValidatorSchnorrSignature};
use tari_engine_types::FromByteType;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    hotstuff::{HotstuffConfig, ProposalValidationError},
    traits::{ConsensusSpec, LeaderStrategy, VoteSignatureService},
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::validations";

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

pub(super) fn check_dummy(candidate_block: &Block) -> Result<(), ProposalValidationError> {
    if candidate_block.signature().is_some() {
        return Err(ProposalValidationError::DummyBlockWithSignature {
            block_id: *candidate_block.id(),
        });
    }
    if !candidate_block.commands().is_empty() {
        return Err(ProposalValidationError::DummyBlockWithCommands {
            block_id: *candidate_block.id(),
        });
    }
    Ok(())
}

pub(super) fn check_network(candidate_block: &Block, network: Network) -> Result<(), ProposalValidationError> {
    if candidate_block.network() != network {
        return Err(ProposalValidationError::InvalidNetwork {
            block_network: candidate_block.network().to_string(),
            expected_network: network.to_string(),
            block_id: *candidate_block.id(),
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

pub(super) fn check_proposed_by_leader<TAddr: DerivableFromPublicKey, TLeaderStrategy: LeaderStrategy<TAddr>>(
    leader_strategy: &TLeaderStrategy,
    local_committee: &Committee<TAddr>,
    header: &BlockHeader,
) -> Result<(), ProposalValidationError> {
    let (leader, _) = leader_strategy.get_leader(local_committee, header.height());
    let Ok(proposed_by) = RistrettoPublicKey::try_from_byte_type(header.proposed_by()) else {
        return Err(ProposalValidationError::MalformedBlock {
            block_id: *header.id(),
            details: format!(
                "proposed_by {} is not a valid compressed RistrettoPublicKey",
                header.proposed_by()
            ),
        });
    };
    if !leader.eq_to_public_key(&proposed_by) {
        return Err(ProposalValidationError::NotLeader {
            proposed_by: header.proposed_by().to_string(),
            expected_leader: leader.to_string(),
            block_id: *header.id(),
        });
    }
    Ok(())
}

pub(super) fn check_block_signature(header: &BlockHeader) -> Result<(), ProposalValidationError> {
    if header.is_dummy() {
        // Dummy blocks don't have signatures
        return Ok(());
    }
    if header.is_genesis() {
        // Genesis block doesn't have signatures
        return Ok(());
    }
    let validator_signature = header.signature().ok_or(ProposalValidationError::MissingSignature {
        block_id: *header.id(),
        height: header.height(),
    })?;
    let Ok(validator_signature) = ValidatorSchnorrSignature::try_from_byte_type(validator_signature) else {
        return Err(ProposalValidationError::MalformedBlock {
            block_id: *header.id(),
            details: format!(
                "signature {} is not a valid compressed Schnorr signature",
                validator_signature
            ),
        });
    };

    debug!(
        target: LOG_TARGET,
        "Validating signature block_id={}, P={}, R={}",
        header.id(),
        header.proposed_by(),
        validator_signature.get_public_nonce(),
    );

    let Ok(proposed_by) = RistrettoPublicKey::try_from_byte_type(header.proposed_by()) else {
        return Err(ProposalValidationError::MalformedBlock {
            block_id: *header.id(),
            details: format!(
                "proposed_by {} is not a valid compressed RistrettoPublicKey",
                header.proposed_by()
            ),
        });
    };

    if !validator_signature.verify(&proposed_by, header.id()) {
        return Err(ProposalValidationError::InvalidSignature {
            block_id: *header.id(),
            height: header.height(),
        });
    }
    Ok(())
}

pub(super) fn check_quorum_certificate<TConsensusSpec: ConsensusSpec>(
    candidate_block: &Block,
    committee: &Committee<TConsensusSpec::Addr>,
    signing_service: &TConsensusSpec::SignatureService,
) -> Result<(), ProposalValidationError> {
    let qc = candidate_block.justify();
    if candidate_block.height() <= qc.block_height() {
        return Err(ProposalValidationError::CandidateBlockNotHigherThanJustify {
            justify_block_height: qc.block_height(),
            candidate_block_height: candidate_block.height(),
        });
    }

    check_quorum_certificate_signatures::<TConsensusSpec>(qc, committee, signing_service)?;

    Ok(())
}

/// Validates the signatures of the quorum certificate.
// pub because used in on receive NEWVIEW
pub fn check_quorum_certificate_signatures<TConsensusSpec: ConsensusSpec>(
    qc: &QuorumCertificate,
    committee: &Committee<TConsensusSpec::Addr>,
    vote_signing_service: &TConsensusSpec::SignatureService,
) -> Result<(), ProposalValidationError> {
    if qc.justifies_zero_block() {
        // TODO: This is potentially dangerous. There should be a check
        // to make sure this is the start of the chain.

        return Ok(());
    }

    if qc.signatures().len() < committee.quorum_threshold() {
        return Err(ProposalValidationError::QuorumWasNotReached {
            qc: *qc.id(),
            got: qc.signatures().len(),
            required: committee.quorum_threshold(),
        });
    }

    let mut check_dups = HashSet::with_capacity(qc.signatures().len());
    for signature in qc.signatures() {
        if !committee.contains_public_key(signature.public_key()) {
            return Err(ProposalValidationError::ValidatorNotInCommittee {
                validator: signature.public_key().to_string(),
                details: format!(
                    "QC signed with validator {} that is not in committee {}",
                    signature.public_key(),
                    qc.shard_group(),
                ),
            });
        }
        if !check_dups.insert(signature.public_key()) {
            return Err(ProposalValidationError::QcDuplicateSignature {
                qc: *qc.id(),
                validator: *signature.public_key(),
            });
        }
        let message = vote_signing_service.create_message(qc.block_id(), &qc.decision());
        if !signature.verify(message) {
            return Err(ProposalValidationError::QcInvalidSignature { qc: *qc.id() });
        }
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
