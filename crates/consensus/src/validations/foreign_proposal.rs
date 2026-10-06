//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use log::*;
use tari_epoch_manager::EpochManagerReader;
use tari_ootle_common_types::{VotePower, committee::Committee, optional::Optional};
use tari_ootle_storage::consensus_models::{CommandsCommitProof, ForeignProposal};
use tari_ootle_transaction::Network;

use crate::{
    hotstuff::{HotStuffError, HotstuffConfig, ProposalValidationError},
    traits::ConsensusSpec,
};

const LOG_TARGET: &str = "tari::ootle::consensus::validations::foreign_proposal";

/// Returns the committee of the shard group the proposal's header names, in the proposal's epoch. The proposal is
/// processed as the evidence and pledges of that shard group, so its proof must verify against this committee.
///
/// Returns `None` when no committee is assigned to exactly that shard group in that epoch.
pub async fn resolve_foreign_committee<TEpochManager: EpochManagerReader>(
    epoch_manager: &TEpochManager,
    proposal: &ForeignProposal,
) -> Result<Option<Arc<Committee<TEpochManager::Addr>>>, HotStuffError> {
    let shard_group = proposal
        .shard_group_checked()
        .ok_or_else(|| ProposalValidationError::InvalidShardGroup {
            block_id: proposal.calculate_block_id(),
            shard_group: proposal.shard_group_unchecked(),
            details: "Foreign proposal header names a shard group with invalid bounds".to_string(),
        })?;

    let committee = epoch_manager
        .get_committee_by_shard_group(proposal.epoch(), shard_group)
        .await
        .optional()?;
    if committee.is_none() {
        warn!(
            target: LOG_TARGET,
            "❌ Foreign proposal block {} names shard group {} which has no committee in epoch {}",
            proposal,
            shard_group,
            proposal.epoch(),
        );
    }
    Ok(committee)
}

pub fn check_foreign_proposal<TConsensusSpec: ConsensusSpec>(
    proposal: &ForeignProposal,
    foreign_committee: &Committee<TConsensusSpec::Addr>,
    config: &HotstuffConfig,
) -> Result<(), HotStuffError> {
    check_network(proposal, config.network)?;
    check_proposer_in_committee(proposal, foreign_committee)?;
    check_header(proposal)?;
    check_commit_proof::<TConsensusSpec>(proposal.commit_proof(), foreign_committee)?;
    Ok(())
}

fn check_proposer_in_committee<TAddr: PartialEq>(
    proposal: &ForeignProposal,
    foreign_committee: &Committee<TAddr>,
) -> Result<(), ProposalValidationError> {
    let proposed_by = proposal.proposed_by();
    if !foreign_committee.contains_public_key(&proposed_by) {
        return Err(ProposalValidationError::ValidatorNotInCommittee {
            validator: proposed_by.to_string(),
            details: format!(
                "Foreign proposal {} was proposed by a validator outside the committee of shard group {}",
                proposal.calculate_block_id(),
                proposal.shard_group_unchecked(),
            ),
        });
    }
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
    proof.validate_committed(quorum_threshold, &|pk| {
        Ok(foreign_committee
            .get_power_by_public_key(pk)
            .unwrap_or_else(VotePower::zero))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::CompressedPublicKey;
    use tari_crypto::tari_utilities::ByteArray;
    use tari_ootle_common_types::{ShardGroup, committee::CommitteeMember, crypto::create_key_pair_from_seed};
    use tari_ootle_storage::consensus_models::BlockPledge;
    use tari_sidechain::{SidechainBlockCommitProof, SidechainBlockHeader};
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

    use super::*;

    fn public_key(seed: u8) -> RistrettoPublicKeyBytes {
        let (_, pk) = create_key_pair_from_seed(seed);
        RistrettoPublicKeyBytes::from_bytes(pk.as_bytes()).unwrap()
    }

    fn committee(seeds: &[u8]) -> Committee<String> {
        Committee::new(
            seeds
                .iter()
                .map(|seed| CommitteeMember {
                    address: format!("vn{seed}"),
                    public_key: public_key(*seed),
                    vote_power: VotePower::of(1),
                })
                .collect(),
        )
    }

    fn proposal(proposer_seed: u8, shard_group: ShardGroup) -> ForeignProposal {
        let (_, proposer) = create_key_pair_from_seed(proposer_seed);
        let commit_proof = CommandsCommitProof::new_latest(vec![], SidechainBlockCommitProof {
            header: SidechainBlockHeader {
                network: Network::LocalNet.as_byte(),
                protocol_version: 0,
                parent_id: Default::default(),
                justify_id: Default::default(),
                height: 1,
                epoch: 1,
                epoch_hash: Default::default(),
                shard_group: tari_sidechain::ShardGroup {
                    start: shard_group.start().as_u32(),
                    end_inclusive: shard_group.end().as_u32(),
                },
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

    fn shard_group_b() -> ShardGroup {
        ShardGroup::new(129u32, 256u32)
    }

    #[test]
    fn it_rejects_a_proposer_outside_the_named_shard_groups_committee() {
        let b = shard_group_b();
        let committee_b = committee(&[11, 12, 13]);
        // The header names shard group B, but the proposer is a validator outside B's committee
        let proposal = proposal(1, b);

        let err = check_proposer_in_committee(&proposal, &committee_b).unwrap_err();
        assert!(
            matches!(err, ProposalValidationError::ValidatorNotInCommittee { .. }),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn it_accepts_a_proposer_inside_the_named_shard_groups_committee() {
        let b = shard_group_b();
        let committee_b = committee(&[11, 12, 13]);
        let proposal = proposal(12, b);

        check_proposer_in_committee(&proposal, &committee_b).unwrap();
    }
}
