//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::ProposalVote;
use tari_ootle_common_types::{committee::Committee, NodeHeight};

use super::vote_collector::ProposalVoteCollector;
use crate::{
    hotstuff::{epoch_state::EpochState, error::HotStuffError, pacemaker_handle::PaceMakerHandle},
    messages::VoteMessage,
    tracing::TraceTimer,
    traits::{ConsensusSpec, LeaderStrategy},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_receive_vote";

pub struct OnReceiveVoteHandler<TConsensusSpec: ConsensusSpec> {
    pacemaker: PaceMakerHandle,
    vote_collector: ProposalVoteCollector<TConsensusSpec>,
    leader_strategy: TConsensusSpec::LeaderStrategy,
    local_addr: TConsensusSpec::Addr,
}

impl<TConsensusSpec> OnReceiveVoteHandler<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        pacemaker: PaceMakerHandle,
        vote_collector: ProposalVoteCollector<TConsensusSpec>,
        local_addr: TConsensusSpec::Addr,
        leader_strategy: TConsensusSpec::LeaderStrategy,
    ) -> Self {
        Self {
            vote_collector,
            pacemaker,
            local_addr,
            leader_strategy,
        }
    }

    pub async fn handle(
        &self,
        from: TConsensusSpec::Addr,
        current_height: NodeHeight,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        message: VoteMessage,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::info(LOG_TARGET, "OnReceiveVote");
        if !self.check_leader_for_vote(epoch_state.local_committee(), &message.vote) {
            return Ok(());
        }
        match self
            .vote_collector
            .check_and_collect_vote(from, current_height, epoch_state, message.vote)
            .await
        {
            Ok(Some((_, high_qc))) => {
                // Reset the leader timeout (not the block timer) - this mitigates the chance of our node sending a
                // NEWVIEW just before we are ready to propose
                self.pacemaker.reset_leader_timeout(high_qc.block_height()).await?;
                // We've reached quorum, trigger a check to see if we should propose immediately
                self.pacemaker.beat();
            },
            Ok(None) => {
                // No quorum yet, do nothing
            },
            Err(err) => {
                // We don't want bad vote messages to kick us out of running mode
                warn!(target: LOG_TARGET, "❌ Error handling vote: {}", err);
            },
        }
        Ok(())
    }

    fn check_leader_for_vote(&self, committee: &Committee<TConsensusSpec::Addr>, vote: &ProposalVote) -> bool {
        let (addr, _) = self.leader_strategy.get_leader(committee, vote.block_height);
        if *addr != self.local_addr {
            warn!(target: LOG_TARGET, "❌ Discarding {vote}: We are not the leader for this vote (expected leader {addr})");
            return false;
        }

        true
    }
}
