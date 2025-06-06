//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common::configuration::Network;
use tari_ootle_common_types::committee::Committee;
use tari_ootle_storage::consensus_models::{CommandsCommitProof, ForeignProposal};

use crate::{
    hotstuff::{HotStuffError, HotstuffConfig, ProposalValidationError},
    traits::ConsensusSpec,
};

pub fn check_foreign_proposal<TConsensusSpec: ConsensusSpec>(
    proposal: &ForeignProposal,
    foreign_committee: &Committee<TConsensusSpec::Addr>,
    config: &HotstuffConfig,
) -> Result<(), HotStuffError> {
    check_network(proposal, config.network)?;
    check_header(proposal)?;
    check_commit_proof::<TConsensusSpec>(proposal.commit_proof(), foreign_committee)?;
    Ok(())
}

fn check_header(proposal: &ForeignProposal) -> Result<(), ProposalValidationError> {
    proposal.commit_proof().validate_header()?;
    Ok(())
}

pub(super) fn check_network(proposal: &ForeignProposal, network: Network) -> Result<(), ProposalValidationError> {
    if proposal.network_byte() != network.as_byte() {
        return Err(ProposalValidationError::InvalidNetwork {
            block_network: Network::try_from(proposal.network_byte())
                .map(|n| n.to_string())
                .unwrap_or_else(|_| format!("<unknown> byte: {}", proposal.network_byte())),
            expected_network: network.to_string(),
            block_id: proposal.calculate_block_id(),
        });
    }
    Ok(())
}

pub fn check_commit_proof<TConsensusSpec: ConsensusSpec>(
    proof: &CommandsCommitProof,
    foreign_committee: &Committee<TConsensusSpec::Addr>,
) -> Result<(), ProposalValidationError> {
    let quorum_threshold = foreign_committee.quorum_threshold();
    proof.validate_committed(quorum_threshold, &|pk| Ok(foreign_committee.contains_public_key(pk)))?;
    Ok(())
}
