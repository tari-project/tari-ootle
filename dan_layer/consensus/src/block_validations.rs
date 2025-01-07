//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use log::{debug, warn};
use tari_common::configuration::Network;
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_dan_common_types::{committee::Committee, DerivableFromPublicKey, Epoch, ExtraFieldKey};
use tari_dan_storage::consensus_models::{Block, QuorumCertificate};
use tari_epoch_manager::EpochManagerReader;

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
) -> Result<(), HotStuffError> {
    check_proposal::<TConsensusSpec>(
        block,
        committee_for_block,
        vote_signing_service,
        leader_strategy,
        config,
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
) -> Result<(), HotStuffError> {
    // TODO: in order to do the base layer block has validation, we need to ensure that we have synced to the tip.
    //       If not, we need some strategy for "parking" the blocks until we are at least at the provided hash or the
    //       tip. Without this, the check has a race condition between the base layer scanner and consensus.
    //       A simpler suggestion is to use the BL epoch block which does not change within epochs
    // check_base_layer_block_hash::<TConsensusSpec>(block, epoch_manager, config).await?;
    check_network(block, config.network)?;
    if block.is_genesis() {
        return Err(ProposalValidationError::ProposingGenesisBlock {
            proposed_by: block.proposed_by().to_string(),
            hash: *block.id(),
        }
        .into());
    }
    check_sidechain_id(block, config)?;
    if block.is_dummy() {
        check_dummy(block)?;
    }
    check_proposed_by_leader(leader_strategy, committee_for_block, block)?;
    check_signature(block)?;
    check_block(block)?;
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

// TODO: remove allow(dead_code)
#[allow(dead_code)]
pub async fn check_base_layer_block_hash<TConsensusSpec: ConsensusSpec>(
    block: &Block,
    epoch_manager: &TConsensusSpec::EpochManager,
    config: &HotstuffConfig,
) -> Result<(), HotStuffError> {
    if block.is_genesis() {
        return Ok(());
    }
    // Check if know the base layer block hash
    let base_layer_block_height = epoch_manager
        .get_base_layer_block_height(*block.base_layer_block_hash())
        .await?
        .ok_or_else(|| ProposalValidationError::BlockHashNotFound {
            hash: *block.base_layer_block_hash(),
        })?;
    // Check if the base layer block height is matching the base layer block hash
    if base_layer_block_height != block.base_layer_block_height() {
        Err(ProposalValidationError::BlockHeightMismatch {
            height: block.base_layer_block_height(),
            real_height: base_layer_block_height,
        })?;
    }
    // Check if the base layer block height is within the acceptable range
    let current_height = epoch_manager.current_base_layer_block_info().await?.0;
    // TODO: uncomment this when the sync information is available here, otherwise during sync this will fail
    // if base_layer_block_height + config.max_base_layer_blocks_behind < current_height {
    //     Err(ProposalValidationError::BlockHeightTooSmall {
    //         proposed: base_layer_block_height,
    //         current: current_height,
    //     })?;
    // }
    if base_layer_block_height > current_height + config.consensus_constants.max_base_layer_blocks_ahead {
        Err(ProposalValidationError::BlockHeightTooHigh {
            proposed: base_layer_block_height,
            current: current_height,
        })?;
    }
    // if block.is_epoch_end() && !epoch_manager.is_last_block_of_epoch(base_layer_block_height).await? {
    //     Err(ProposalValidationError::NotLastBlockOfEpoch {
    //         block_id: *block.id(),
    //         base_layer_block_height,
    //     })?;
    // }
    Ok(())
}

pub fn check_proposed_by_leader<TAddr: DerivableFromPublicKey, TLeaderStrategy: LeaderStrategy<TAddr>>(
    leader_strategy: &TLeaderStrategy,
    local_committee: &Committee<TAddr>,
    candidate_block: &Block,
) -> Result<(), ProposalValidationError> {
    let (leader, _) = leader_strategy.get_leader(local_committee, candidate_block.height());
    if !leader.eq_to_public_key(candidate_block.proposed_by()) {
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
    debug!(
        target: LOG_TARGET,
        "Validating signature block_id={}, P={}, R={}",
        candidate_block.id(),
        candidate_block.proposed_by(),
        validator_signature.get_public_nonce(),
    );
    if !validator_signature.verify(candidate_block.proposed_by(), candidate_block.id()) {
        return Err(ProposalValidationError::InvalidSignature {
            block_id: *candidate_block.id(),
            height: candidate_block.height(),
        });
    }
    Ok(())
}

pub fn check_block(candidate_block: &Block) -> Result<(), ProposalValidationError> {
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
                validator: signature.public_key().clone(),
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
        let sidechain_id = RistrettoPublicKey::from_canonical_bytes(sidechain_id_bytes).map_err(|e| {
            ProposalValidationError::InvalidSidechainId {
                block_id: *candidate_block.id(),
                reason: e.to_string(),
            }
        })?;

        // The sidechain id must match the sidechain of the current network
        if sidechain_id != *expected_sidechain_id {
            return Err(ProposalValidationError::MismatchedSidechainId {
                block_id: *candidate_block.id(),
                expected_sidechain_id: expected_sidechain_id.clone(),
                sidechain_id,
            }
            .into());
        }
    }

    Ok(())
}
