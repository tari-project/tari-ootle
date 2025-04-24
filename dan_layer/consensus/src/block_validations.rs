//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use log::{debug, warn};
use tari_common::configuration::Network;
use tari_common_types::types::FixedHash;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_dan_common_types::{committee::Committee, DerivableFromPublicKey, Epoch, ExtraFieldKey};
use tari_dan_storage::consensus_models::{Block, QuorumCertificate, ValidatorSchnorrSignature};
use tari_engine_types::FromByteType;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    hotstuff::{HotStuffError, HotstuffConfig, ProposalValidationError},
    traits::{ConsensusSpec, LeaderStrategy, VoteSignatureService},
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::block_validations";
pub fn check_local_proposal<TConsensusSpec: ConsensusSpec>(
    current_epoch: Epoch,
    block: &Block,
    committee_for_block: &Committee<TConsensusSpec::Addr>,
    vote_signing_service: &TConsensusSpec::SignatureService,
    leader_strategy: &TConsensusSpec::LeaderStrategy,
    config: &HotstuffConfig,
    expected_epoch_hash: &FixedHash,
) -> Result<(), HotStuffError> {
    check_proposal::<TConsensusSpec>(
        block,
        committee_for_block,
        vote_signing_service,
        leader_strategy,
        config,
        expected_epoch_hash,
    )?;
    // This proposal is valid, if it is for an epoch ahead of us, we need to sync
    check_current_epoch(block, current_epoch)?;
    Ok(())
}

pub fn check_proposal<TConsensusSpec: ConsensusSpec>(
    block: &Block,
    committee_for_block: &Committee<TConsensusSpec::Addr>,
    vote_signing_service: &TConsensusSpec::SignatureService,
    leader_strategy: &TConsensusSpec::LeaderStrategy,
    config: &HotstuffConfig,
    expected_epoch_hash: &FixedHash,
) -> Result<(), HotStuffError> {
    check_network(block, config.network)?;
    if block.is_genesis() {
        return Err(ProposalValidationError::ProposingGenesisBlock {
            proposed_by: block.proposed_by().to_string(),
            hash: *block.id(),
        }
        .into());
    }
    check_epoch_hash(block, expected_epoch_hash)?;
    check_sidechain_id(block, config)?;
    check_block_height(block)?;
    // TODO: we should never have to validate a dummy, they should always be generated locally
    if block.is_dummy() {
        check_dummy(block)?;
    }
    check_proposed_by_leader(leader_strategy, committee_for_block, block)?;
    check_signature(block)?;
    check_quorum_certificate::<TConsensusSpec>(block.justify(), committee_for_block, vote_signing_service)?;
    Ok(())
}

pub fn check_current_epoch(candidate_block: &Block, current_epoch: Epoch) -> Result<(), ProposalValidationError> {
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

pub fn check_dummy(candidate_block: &Block) -> Result<(), ProposalValidationError> {
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

pub fn check_network(candidate_block: &Block, network: Network) -> Result<(), ProposalValidationError> {
    if candidate_block.network() != network {
        return Err(ProposalValidationError::InvalidNetwork {
            block_network: candidate_block.network().to_string(),
            expected_network: network.to_string(),
            block_id: *candidate_block.id(),
        });
    }
    Ok(())
}

pub fn check_epoch_hash(block: &Block, expected_epoch_hash: &FixedHash) -> Result<(), HotStuffError> {
    if block.epoch_hash() != expected_epoch_hash {
        Err(ProposalValidationError::InvalidEpochHash {
            epoch: block.epoch(),
            local_epoch_hash: *expected_epoch_hash,
            invalid_epoch_hash: *block.epoch_hash(),
            block_id: *block.id(),
        })?;
    }

    Ok(())
}

pub fn check_proposed_by_leader<TAddr: DerivableFromPublicKey, TLeaderStrategy: LeaderStrategy<TAddr>>(
    leader_strategy: &TLeaderStrategy,
    local_committee: &Committee<TAddr>,
    candidate_block: &Block,
) -> Result<(), ProposalValidationError> {
    let (leader, _) = leader_strategy.get_leader(local_committee, candidate_block.height());
    let Ok(proposed_by) = RistrettoPublicKey::try_from_byte_type(candidate_block.proposed_by()) else {
        return Err(ProposalValidationError::MalformedBlock {
            block_id: *candidate_block.id(),
            details: format!(
                "proposed_by {} is not a valid compressed RistrettoPublicKey",
                candidate_block.proposed_by()
            ),
        });
    };
    if !leader.eq_to_public_key(&proposed_by) {
        return Err(ProposalValidationError::NotLeader {
            proposed_by: candidate_block.proposed_by().to_string(),
            expected_leader: leader.to_string(),
            block_id: *candidate_block.id(),
        });
    }
    Ok(())
}

pub fn check_signature(candidate_block: &Block) -> Result<(), ProposalValidationError> {
    if candidate_block.is_dummy() {
        // Dummy blocks don't have signatures
        return Ok(());
    }
    if candidate_block.is_genesis() {
        // Genesis block doesn't have signatures
        return Ok(());
    }
    let validator_signature = candidate_block
        .signature()
        .ok_or(ProposalValidationError::MissingSignature {
            block_id: *candidate_block.id(),
            height: candidate_block.height(),
        })?;
    let Ok(validator_signature) = ValidatorSchnorrSignature::try_from_byte_type(validator_signature) else {
        return Err(ProposalValidationError::MalformedBlock {
            block_id: *candidate_block.id(),
            details: format!(
                "signature {} is not a valid compressed Schnorr signature",
                validator_signature
            ),
        });
    };

    debug!(
        target: LOG_TARGET,
        "Validating signature block_id={}, P={}, R={}",
        candidate_block.id(),
        candidate_block.proposed_by(),
        validator_signature.get_public_nonce(),
    );

    let Ok(proposed_by) = RistrettoPublicKey::try_from_byte_type(candidate_block.proposed_by()) else {
        return Err(ProposalValidationError::MalformedBlock {
            block_id: *candidate_block.id(),
            details: format!(
                "proposed_by {} is not a valid compressed RistrettoPublicKey",
                candidate_block.proposed_by()
            ),
        });
    };

    if !validator_signature.verify(&proposed_by, candidate_block.id()) {
        return Err(ProposalValidationError::InvalidSignature {
            block_id: *candidate_block.id(),
            height: candidate_block.height(),
        });
    }
    Ok(())
}

pub fn check_block_height(candidate_block: &Block) -> Result<(), ProposalValidationError> {
    let qc = candidate_block.justify();
    if candidate_block.height() <= qc.block_height() {
        return Err(ProposalValidationError::CandidateBlockNotHigherThanJustify {
            justify_block_height: qc.block_height(),
            candidate_block_height: candidate_block.height(),
        });
    }

    Ok(())
}

pub fn check_quorum_certificate<TConsensusSpec: ConsensusSpec>(
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

pub fn check_sidechain_id(candidate_block: &Block, config: &HotstuffConfig) -> Result<(), HotStuffError> {
    // We only require the sidechain id on the genesis block
    if !candidate_block.is_genesis() {
        return Ok(());
    }

    // If we are using a sidechain id in the network, we need to check it matches the candidate block one
    if let Some(expected_sidechain_id) = &config.sidechain_id {
        // Extract the sidechain id from the candidate block
        let extra_data = candidate_block.extra_data();
        let sidechain_id_bytes = extra_data.get(&ExtraFieldKey::SidechainId).ok_or::<HotStuffError>(
            ProposalValidationError::InvalidSidechainId {
                block_id: *candidate_block.id(),
                reason: "SidechainId key not present".to_owned(),
            }
            .into(),
        )?;
        let sidechain_id = RistrettoPublicKeyBytes::from_bytes(sidechain_id_bytes.as_ref()).map_err(|e| {
            ProposalValidationError::InvalidSidechainId {
                block_id: *candidate_block.id(),
                reason: e.to_string(),
            }
        })?;

        // The sidechain id must match the sidechain of the current network
        if sidechain_id != *expected_sidechain_id {
            return Err(ProposalValidationError::MismatchedSidechainId {
                block_id: *candidate_block.id(),
                expected_sidechain_id: *expected_sidechain_id,
                sidechain_id,
            }
            .into());
        }
    }

    Ok(())
}
