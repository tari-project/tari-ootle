//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::{LastExecuted, ProposalVote};
use tari_ootle_common_types::{Epoch, NodeHeight, optional::Optional};
use tari_ootle_storage::{StateStore, consensus_models::BookkeepingModel};

use super::vote_collector::ProposalVoteCollector;
use crate::{
    hotstuff::{
        HotstuffConfig,
        LeaderSkipSet,
        epoch_state::EpochState,
        error::HotStuffError,
        pacemaker_handle::PaceMakerHandle,
    },
    messages::VoteMessage,
    tracing::TraceTimer,
    traits::ConsensusSpec,
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::on_receive_vote";

pub struct OnReceiveVoteHandler<TConsensusSpec: ConsensusSpec> {
    config: HotstuffConfig,
    store: TConsensusSpec::StateStore,
    pacemaker: PaceMakerHandle,
    vote_collector: ProposalVoteCollector<TConsensusSpec>,
    leader_strategy: TConsensusSpec::LeaderStrategy,
    local_addr: TConsensusSpec::Addr,
    /// The skip set for the view the last vote was for. Every vote for one block asks the same
    /// question of the same anchor, and answering it from the store costs a lookup per committee
    /// member, so the answer for a view is loaded once.
    ///
    /// It is keyed by what this node had committed when it was loaded as well as by the view. A vote
    /// for a block can be handled before this node has processed that block - the proposer broadcasts
    /// to the whole committee at once - and the liveness log then answers from below the anchor
    /// rather than at it. Committing more is what makes that answer authoritative, so the set is
    /// reloaded once it does.
    cached_skip_set: Option<CachedSkipSet>,
}

impl<TConsensusSpec> OnReceiveVoteHandler<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        config: HotstuffConfig,
        store: TConsensusSpec::StateStore,
        pacemaker: PaceMakerHandle,
        vote_collector: ProposalVoteCollector<TConsensusSpec>,
        local_addr: TConsensusSpec::Addr,
        leader_strategy: TConsensusSpec::LeaderStrategy,
    ) -> Self {
        Self {
            config,
            store,
            vote_collector,
            pacemaker,
            local_addr,
            leader_strategy,
            cached_skip_set: None,
        }
    }

    pub async fn handle(
        &mut self,
        from: TConsensusSpec::Addr,
        current_height: NodeHeight,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        message: VoteMessage,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::info(LOG_TARGET, "OnReceiveVote");
        if !self.is_leader_for_vote(epoch_state, &message.vote)? {
            return Ok(());
        }
        match self
            .vote_collector
            .check_and_collect_vote(from, current_height, epoch_state, message.vote)
            .await
        {
            Ok(Some((_, high_pc))) => {
                // Reset the leader timeout (not the block timer) - this mitigates the chance of our node sending a
                // NEWVIEW just before we are ready to propose
                self.pacemaker.reset_leader_timeout(&high_pc).await?;
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

    /// A vote is addressed to the validator that proposes the block certifying the voted block, so
    /// that block's height anchors the liveness state that decides who that is - the same anchor the
    /// voter used.
    fn is_leader_for_vote(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        vote: &ProposalVote,
    ) -> Result<bool, HotStuffError> {
        let committee = epoch_state.local_committee();
        self.load_skip_set_for_view(epoch_state, vote.block_height)?;
        let cached = self.cached_skip_set.as_ref().expect("loaded above");
        let (addr, _) = cached
            .skip_set
            .effective_leader(&self.leader_strategy, committee, vote.block_height);
        if *addr != self.local_addr {
            warn!(target: LOG_TARGET, "❌ Discarding {vote}: We are not the leader for this vote (expected leader {addr})");
            return Ok(false);
        }

        Ok(true)
    }

    fn load_skip_set_for_view(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        view_height: NodeHeight,
    ) -> Result<(), HotStuffError> {
        let epoch = epoch_state.epoch();
        let committed = self.store.with_read_tx(|tx| {
            let committed = LastExecuted::get(tx, epoch)
                .optional()?
                .map(|last| last.height)
                .unwrap_or_default();
            Ok::<_, HotStuffError>(committed)
        })?;

        if self.cached_skip_set.as_ref().is_some_and(|cached| {
            cached.epoch == epoch && cached.view_height == view_height && cached.committed == committed
        }) {
            return Ok(());
        }

        let skip_set = self.store.with_read_tx(|tx| {
            LeaderSkipSet::load_for_justify(
                tx,
                epoch,
                view_height,
                epoch_state.local_committee(),
                &self.config.consensus_constants.liveness_thresholds(),
            )
        })?;
        self.cached_skip_set = Some(CachedSkipSet {
            epoch,
            view_height,
            committed,
            skip_set,
        });

        Ok(())
    }
}

struct CachedSkipSet {
    epoch: Epoch,
    view_height: NodeHeight,
    /// What this node had committed when the set was loaded.
    committed: NodeHeight,
    skip_set: LeaderSkipSet,
}
