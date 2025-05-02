//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::FixedHash;
use tari_dan_common_types::{
    committee::{Committee, CommitteeInfo},
    Epoch,
};
use tari_dan_storage::consensus_models::{Block, BlockHeader};

use super::common::{
    check_block_signature,
    check_current_epoch,
    check_dummy,
    check_epoch_hash,
    check_network,
    check_proposed_by_leader,
    check_quorum_certificate,
    check_shard_group_bounds,
    check_shard_group_matches,
    check_sidechain_id,
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
    check_shard_group_matches(block.header(), local_committee_info.shard_group())?;
    // This proposal is valid, if it is for an epoch ahead of us, we need to sync
    check_current_epoch(block, current_epoch)?;
    Ok(())
}
fn check_proposal<TConsensusSpec: ConsensusSpec>(
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
    check_header::<TConsensusSpec>(
        block.header(),
        expected_epoch_hash,
        config,
        leader_strategy,
        committee_for_block,
    )?;
    check_quorum_certificate::<TConsensusSpec>(block, committee_for_block, vote_signing_service)?;
    // TODO: we should immediately reject dummy blocks, they should always be generated locally. Currently required to
    // trigger a view change on catch up.
    if block.is_dummy() {
        check_dummy(block)?;
    }

    Ok(())
}

fn check_header<TConsensusSpec: ConsensusSpec>(
    header: &BlockHeader,
    expected_epoch_hash: &FixedHash,
    config: &HotstuffConfig,
    leader_strategy: &TConsensusSpec::LeaderStrategy,
    committee_for_block: &Committee<TConsensusSpec::Addr>,
) -> Result<(), ProposalValidationError> {
    check_epoch_hash(header, expected_epoch_hash)?;
    check_shard_group_bounds(header, config.consensus_constants.num_preshards)?;
    check_proposed_by_leader(leader_strategy, committee_for_block, header)?;
    check_block_signature(header)?;
    check_sidechain_id(header, config)?;
    Ok(())
}
