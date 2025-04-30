//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common::configuration::Network;
use tari_dan_common_types::committee::Committee;
use tari_dan_storage::consensus_models::{CommandsCommitProof, ForeignProposal};

use crate::{
    hotstuff::{HotStuffError, HotstuffConfig, ProposalValidationError},
    traits::{ConsensusSpec, LeaderStrategy},
};

pub fn check_foreign_proposal<TConsensusSpec: ConsensusSpec>(
    proposal: &ForeignProposal,
    committee: &Committee<TConsensusSpec::Addr>,
    leader_strategy: &TConsensusSpec::LeaderStrategy,
    config: &HotstuffConfig,
) -> Result<(), HotStuffError> {
    check_network(proposal, config.network)?;
    check_header::<TConsensusSpec>(proposal, committee, leader_strategy)?;
    check_commit_proof::<TConsensusSpec>(proposal.commit_proof(), committee)?;
    Ok(())
}

fn check_header<TConsensusSpec: ConsensusSpec>(
    proposal: &ForeignProposal,
    committee: &Committee<TConsensusSpec::Addr>,
    leader_strategy: &TConsensusSpec::LeaderStrategy,
) -> Result<(), ProposalValidationError> {
    let (_, leader) = leader_strategy.get_leader(committee, proposal.height());
    proposal.commit_proof().validate_header(leader)?;
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
    committee: &Committee<TConsensusSpec::Addr>,
) -> Result<(), ProposalValidationError> {
    let quorum_threshold = committee.quorum_threshold();
    proof.validate_committed(quorum_threshold, &|pk| Ok(committee.contains_public_key(pk)))?;
    Ok(())
}
