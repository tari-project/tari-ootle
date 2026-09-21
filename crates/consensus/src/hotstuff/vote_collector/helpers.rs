//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::Vote;
use tari_epoch_manager::EpochManagerReader;
use tari_ootle_common_types::{committee::CommitteeInfo, optional::Optional};
use tari_ootle_storage::{
    StateStore,
    StateStoreReadTransaction,
    StateStoreWriteTransaction,
    consensus_models::VoteEquivocation,
    global::models::ValidatorNode,
};

use crate::{
    hotstuff::HotStuffError,
    traits::{ConsensusSpec, hooks::ConsensusHooks},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::vote_collector";

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

/// Persists equivocation evidence and reports it. Only the first pair per (view, signer) is kept: an
/// equivocator can sign arbitrarily many conflicting votes for one view, and every pair after the
/// first proves exactly what the first one does.
///
/// The read comes first so that the common case — an equivocator that keeps sending — costs a point
/// lookup rather than a write transaction.
pub fn record_equivocation<TConsensusSpec: ConsensusSpec>(
    store: &TConsensusSpec::StateStore,
    hooks: &mut TConsensusSpec::Hooks,
    evidence: VoteEquivocation,
) -> Result<(), HotStuffError> {
    let already_held =
        store.with_read_tx(|tx| tx.vote_equivocation_exists(evidence.epoch, evidence.height, &evidence.public_key))?;
    if already_held {
        debug!(target: LOG_TARGET, "Already hold evidence of {}", evidence);
        return Ok(());
    }

    if !store.with_write_tx(|tx| tx.vote_equivocation_record(&evidence))? {
        return Ok(());
    }

    warn!(target: LOG_TARGET, "🚨 Recorded evidence of {}", evidence);
    hooks.on_vote_equivocation(&evidence);
    Ok(())
}
