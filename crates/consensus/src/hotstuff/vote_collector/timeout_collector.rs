//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_consensus_types::{HighTc, SignedMessage, TimeoutCertificate, TimeoutVote, Vote};
use tari_ootle_common_types::{Epoch, Network, NodeHeight};
use tari_ootle_storage::StateStore;

use super::collector::VoteCollector;
use crate::{
    hotstuff::{epoch_state::EpochState, error::HotStuffError, vote_collector::helpers::check_eligibility},
    tracing::TraceTimer,
    traits::{CertificateStore, ConsensusSpec, ValidatorSignatureVerifierService},
};

const LOG_TARGET: &str = "tari::ootle::consensus::hotstuff::timeout_collector";

#[derive(Clone)]
pub struct TimeoutVoteCollector<TConsensusSpec: ConsensusSpec> {
    network: Network,
    vote_collector: VoteCollector<TimeoutVote>,
    store: TConsensusSpec::StateStore,
    epoch_manager: TConsensusSpec::EpochManager,
    vote_signer_service: TConsensusSpec::SignerService,
}

impl<TConsensusSpec> TimeoutVoteCollector<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        network: Network,
        store: TConsensusSpec::StateStore,
        epoch_manager: TConsensusSpec::EpochManager,
        vote_signer_service: TConsensusSpec::SignerService,
    ) -> Self {
        Self {
            network,
            store,
            vote_collector: VoteCollector::new(),
            epoch_manager,
            vote_signer_service,
        }
    }

    /// Returns Some if quorum is reached
    pub async fn check_and_collect_vote(
        &self,
        from: TConsensusSpec::Addr,
        current_height: NodeHeight,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        vote: TimeoutVote,
    ) -> Result<Option<(TimeoutCertificate, HighTc)>, HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "check_and_collect_vote (TimeoutVote)");
        debug!(
            target: LOG_TARGET,
            "📬 Validating timeout vote message from {from}: {vote}"
        );

        let current_epoch = epoch_state.epoch();
        let local_committee_info = epoch_state.local_committee_info();
        let sender_vn =
            check_eligibility::<TConsensusSpec, _>(&self.epoch_manager, from, &vote, local_committee_info).await?;
        self.validate_vote(current_epoch, &vote)?;
        let sender_leaf_hash = sender_vn.get_node_hash(self.network);
        let height = vote.height;

        let result = self
            .vote_collector
            .collect_vote(
                current_epoch,
                current_height,
                sender_leaf_hash,
                vote,
                epoch_state.local_committee(),
            )
            .await;

        match result {
            Ok(Some((quorum_votes, _))) => {
                let signatures = quorum_votes.into_iter().map(|vote| vote.signature).collect();
                let new_tc = TimeoutCertificate::new(current_epoch, height, signatures);
                let high_tc = self.store.with_write_tx(|tx| new_tc.update_highest(tx))?;
                if new_tc.calculate_id() == *high_tc.id() {
                    info!(target: LOG_TARGET, "🕒️ New HIGH {}", new_tc);
                } else {
                    info!(target: LOG_TARGET, "❓️ New TC from votes {} but it is not the highest TC {}", new_tc, high_tc);
                }

                Ok(Some((new_tc, high_tc)))
            },
            Ok(None) => {
                debug!(target: LOG_TARGET, "🟡 No quorum reached yet for TimeoutVote at height {}", height);
                Ok(None)
            },
            Err(err) => {
                warn!(target: LOG_TARGET, "❌ {}", err);
                // TODO: track equivocation and penalize nodes
                Ok(None)
            },
        }
    }

    fn validate_vote(&self, current_epoch: Epoch, vote: &TimeoutVote) -> Result<(), HotStuffError> {
        if current_epoch != vote.epoch() {
            return Err(HotStuffError::InvalidVote {
                signer_public_key: vote.signature.public_key,
                details: format!(
                    "Our current view is at epoch {} but the vote was for epoch {}",
                    current_epoch,
                    vote.epoch()
                ),
            });
        }

        if !self.vote_signer_service.verify(vote) {
            return Err(HotStuffError::InvalidVoteSignature {
                signer_public_key: *vote.public_key(),
            });
        }

        Ok(())
    }
}
