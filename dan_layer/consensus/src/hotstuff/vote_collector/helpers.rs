//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::Vote;
use tari_dan_common_types::{committee::CommitteeInfo, optional::Optional};
use tari_dan_storage::global::models::ValidatorNode;
use tari_epoch_manager::EpochManagerReader;

use crate::{hotstuff::HotStuffError, traits::ConsensusSpec};

pub async fn check_eligibility<TConsensusSpec: ConsensusSpec, V: Vote>(
    epoch_manager: &TConsensusSpec::EpochManager,
    from: TConsensusSpec::Addr,
    vote: &V,
    local_committee_info: &CommitteeInfo,
) -> Result<ValidatorNode<TConsensusSpec::Addr>, HotStuffError> {
    // Does the vote come from a local committee member?
    let sender_vn = epoch_manager
        .get_validator_node_by_public_key(vote.epoch(), *vote.public_key())
        .await
        .optional()?
        .ok_or_else(|| HotStuffError::ReceivedVoteFromNonCommitteeMember {
            epoch: vote.epoch(),
            sender: from.to_string(),
            context: "VoteReceiver::handle_vote (sender pk not from registered VN)".to_string(),
        })?;

    // Get the sender shard, and check that they are in the local committee
    if !local_committee_info.includes_substate_address(&sender_vn.shard_key) {
        return Err(HotStuffError::ReceivedVoteFromNonCommitteeMember {
            epoch: vote.epoch(),
            sender: sender_vn.address.to_string(),
            context: "VoteReceiver::handle_vote (VN not in local committee)".to_string(),
        });
    }

    Ok(sender_vn)
}
