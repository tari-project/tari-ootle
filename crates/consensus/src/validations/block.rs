//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::fees::ExhaustBurnRate;
use tari_ootle_common_types::{
    Epoch,
    committee::{Committee, CommitteeInfo},
};
use tari_ootle_storage::consensus_models::{Block, BlockHeader};
use tari_sidechain::FixedHash;

use super::common::{
    check_block_signature,
    check_current_epoch,
    check_epoch_hash,
    check_exhaust_burn_rate,
    check_height,
    check_network,
    check_next_epoch_fee_scale,
    check_proposal_certificate,
    check_protocol_version,
    check_shard_group_bounds,
    check_shard_group_matches,
    check_sidechain_id,
    check_timeout_certificate,
};
use crate::{
    hotstuff::{HotStuffError, HotstuffConfig, ProposalValidationError},
    traits::ConsensusSpec,
};

pub fn check_local_proposal<TConsensusSpec: ConsensusSpec>(
    current_epoch: Epoch,
    block: &Block,
    committee_for_block: &Committee<TConsensusSpec::Addr>,
    local_committee_info: &CommitteeInfo,
    vote_signing_service: &TConsensusSpec::SignerService,
    config: &HotstuffConfig,
    expected_epoch_hash: &FixedHash,
    expected_exhaust_burn_rate: ExhaustBurnRate,
) -> Result<(), HotStuffError> {
    check_proposal::<TConsensusSpec>(
        block,
        committee_for_block,
        vote_signing_service,
        config,
        expected_epoch_hash,
        expected_exhaust_burn_rate,
    )?;
    check_shard_group_matches(block.header(), local_committee_info.shard_group())?;
    // This proposal is valid, if it is for an epoch ahead of us, we need to sync
    check_current_epoch(block, current_epoch)?;
    Ok(())
}
fn check_proposal<TConsensusSpec: ConsensusSpec>(
    block: &Block,
    committee_for_block: &Committee<TConsensusSpec::Addr>,
    signer_service: &TConsensusSpec::SignerService,
    config: &HotstuffConfig,
    expected_epoch_hash: &FixedHash,
    expected_exhaust_burn_rate: ExhaustBurnRate,
) -> Result<(), HotStuffError> {
    check_header::<TConsensusSpec>(
        block.header(),
        expected_epoch_hash,
        expected_exhaust_burn_rate,
        config,
        signer_service,
    )?;
    check_block(block)?;
    check_next_epoch_fee_scale(block, config.network)?;
    check_proposal_certificate::<TConsensusSpec>(config.network, block, committee_for_block, signer_service)?;
    check_timeout_certificate::<TConsensusSpec>(config.network, block, committee_for_block, signer_service)?;

    Ok(())
}
/// Checks that do not depend on committed state. Whether the proposer is the leader of the view
/// below the block does: it is checked in `validate_local_proposed_block`, where the block's justify
/// is on hand to anchor the liveness state that decides it.
pub(super) fn check_block(block: &Block) -> Result<(), ProposalValidationError> {
    check_height(block)?;
    for atom in block.all_foreign_proposals() {
        if atom.shard_group == block.shard_group() {
            return Err(ProposalValidationError::ForeignProposalFromLocalShardGroup {
                block_id: atom.block_id,
                shard_group: atom.shard_group,
            });
        }
    }
    Ok(())
}

fn check_header<TConsensusSpec: ConsensusSpec>(
    header: &BlockHeader,
    expected_epoch_hash: &FixedHash,
    expected_exhaust_burn_rate: ExhaustBurnRate,
    config: &HotstuffConfig,
    signer_service: &TConsensusSpec::SignerService,
) -> Result<(), ProposalValidationError> {
    check_network(header, config.network)?;
    check_protocol_version(header, config.network)?;
    if header.is_genesis() {
        return Err(ProposalValidationError::ProposingGenesisBlock {
            proposed_by: header.proposed_by().to_string(),
            block_id: *header.id(),
        });
    }

    if header.is_dummy() {
        return Err(ProposalValidationError::ProposingDummyBlock {
            proposed_by: header.proposed_by().to_string(),
            block: header.as_leaf(),
        });
    }
    check_epoch_hash(header, expected_epoch_hash)?;
    check_exhaust_burn_rate(header, expected_exhaust_burn_rate)?;
    check_shard_group_bounds(header, config.consensus_constants.num_preshards)?;
    check_block_signature(header, signer_service)?;
    check_sidechain_id(header, config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tari_consensus_types::{BlockId, ProposalCertificate, ShardGroupAccumulatedData};
    use tari_ootle_common_types::{ExtraData, NodeHeight, ProtocolVersion, ShardGroup};
    use tari_ootle_storage::consensus_models::{BlockCommands, Command, ForeignProposalAtom};
    use tari_ootle_transaction::Network;
    use tari_sidechain::{FixedHash, QuorumDecision};
    use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

    use super::*;

    fn block_in(shard_group: ShardGroup, commands: BlockCommands) -> Block {
        let epoch = Epoch(1);
        let justify = ProposalCertificate::new(
            FixedHash::zero(),
            BlockId::zero(),
            NodeHeight(0),
            epoch,
            shard_group,
            ProposalCertificate::genesis_signatures(),
            QuorumDecision::Accept,
        );
        let header = BlockHeader::create_unsigned(
            Network::LocalNet,
            ProtocolVersion::V0,
            BlockId::zero(),
            justify.calculate_id(),
            None,
            NodeHeight(1),
            epoch,
            shard_group,
            RistrettoPublicKeyBytes::default(),
            FixedHash::zero(),
            &commands,
            0,
            0,
            FixedHash::zero(),
            ShardGroupAccumulatedData::default(),
            ExtraData::new(),
        )
        .unwrap();
        Block::new(header, justify, commands, None)
    }

    #[test]
    fn check_block_rejects_foreign_proposal_naming_local_shard_group() {
        let local_sg = ShardGroup::new(0, 127);
        let foreign_sg = ShardGroup::new(128, 255);
        let foreign_block_id = BlockId::from([0xab; 32]);

        let valid_commands = BlockCommands::init([Command::ForeignProposal(ForeignProposalAtom {
            block_id: foreign_block_id,
            shard_group: foreign_sg,
        })])
        .unwrap();
        assert!(check_block(&block_in(local_sg, valid_commands)).is_ok());

        let self_commands = BlockCommands::init([Command::ForeignProposal(ForeignProposalAtom {
            block_id: foreign_block_id,
            shard_group: local_sg,
        })])
        .unwrap();
        let err = check_block(&block_in(local_sg, self_commands)).unwrap_err();
        assert!(
            matches!(
                err,
                ProposalValidationError::ForeignProposalFromLocalShardGroup {
                    block_id,
                    shard_group,
                } if block_id == foreign_block_id && shard_group == local_sg
            ),
            "{err:?}"
        );
    }
}
