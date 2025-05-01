//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::{HashMap, HashSet};

use log::*;
use tari_dan_common_types::{optional::Optional, NodeHeight};
use tari_dan_storage::{
    consensus_models::{Block, BlockId, LeafBlock, QuorumCertificate},
    StateStore,
};

use super::vote_collector::VoteCollector;
use crate::{
    hotstuff::{
        epoch_state::EpochState,
        error::HotStuffError,
        pacemaker_handle::PaceMakerHandle,
        ProposalValidationError,
    },
    messages::NewViewMessage,
    tracing::TraceTimer,
    traits::{ConsensusSpec, LeaderStrategy},
    validations::check_quorum_certificate_signatures,
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::on_receive_new_view";

pub struct OnReceiveNewViewHandler<TConsensusSpec: ConsensusSpec> {
    local_validator_addr: TConsensusSpec::Addr,
    store: TConsensusSpec::StateStore,
    leader_strategy: TConsensusSpec::LeaderStrategy,
    newview_message_counts: HashMap<(NodeHeight, BlockId), HashSet<TConsensusSpec::Addr>>,
    pacemaker: PaceMakerHandle,
    vote_collector: VoteCollector<TConsensusSpec>,
}

impl<TConsensusSpec> OnReceiveNewViewHandler<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        local_validator_addr: TConsensusSpec::Addr,
        store: TConsensusSpec::StateStore,
        leader_strategy: TConsensusSpec::LeaderStrategy,
        pacemaker: PaceMakerHandle,
        vote_receiver: VoteCollector<TConsensusSpec>,
    ) -> Self {
        Self {
            local_validator_addr,
            store,
            leader_strategy,
            newview_message_counts: HashMap::default(),
            pacemaker,
            vote_collector: vote_receiver,
        }
    }

    pub(super) fn clear_new_views(&mut self) {
        self.newview_message_counts.clear();
    }

    fn collect_new_views(
        &mut self,
        from: TConsensusSpec::Addr,
        new_height: NodeHeight,
        high_qc: &QuorumCertificate,
    ) -> usize {
        self.newview_message_counts
            .retain(|(height, _), _| *height >= new_height);
        if self.newview_message_counts.len() <= 10 && self.newview_message_counts.capacity() > 10 {
            self.newview_message_counts.shrink_to_fit();
        }
        let entry = self
            .newview_message_counts
            .entry((new_height, *high_qc.block_id()))
            .or_default();
        entry.insert(from);
        entry.len()
    }

    #[allow(clippy::too_many_lines)]
    pub async fn handle(
        &mut self,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        current_height: NodeHeight,
        from: TConsensusSpec::Addr,
        message: NewViewMessage,
    ) -> Result<(), HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "OnReceiveNewView");

        let NewViewMessage {
            high_qc,
            new_height,
            last_vote,
            ..
        } = message;
        info!(
            target: LOG_TARGET,
            "🌟 NEWVIEW from {from} with new height {new_height} with qc {high_qc}",
        );
        if new_height < current_height {
            warn!(target: LOG_TARGET, "❌ Ignoring NEWVIEW for {new_height} less than the current {current_height}.");
            return Ok(());
        }

        let is_qc_valid = self.store.with_read_tx(|tx| {
            // If we already have this QC (locally calculated hash matches), we do not need to validate this again
            // if !high_qc.exists(tx)? {
            if let Err(err) = self.validate_qc(&high_qc, epoch_state, self.vote_collector.signing_service()) {
                warn!(target: LOG_TARGET, "❌ NEWVIEW: Invalid QC: {}", err);
                return Ok(false);
            }
            // }

            if !Block::record_exists(tx, high_qc.block_id())? {
                // Sync if we do not have the block for this valid QC
                let local_height = LeafBlock::get(tx, epoch_state.epoch())
                    .optional()?
                    .map(|leaf| leaf.height())
                    .unwrap_or_default();
                return Err(HotStuffError::FallenBehind {
                    local_height,
                    qc_height: high_qc.block_height(),
                });
            }

            Ok(true)
        })?;

        if !is_qc_valid {
            return Ok(());
        }

        // Check if we are the leader for the view after new_height. We'll set our local view height to the new_height
        // if quorum is reached and propose a block at new_height + 1.
        let (leader, _) = self
            .leader_strategy
            .get_leader_for_next_height(epoch_state.local_committee(), new_height);

        if *leader != self.local_validator_addr {
            warn!(target: LOG_TARGET, "❌ NEWVIEW failed, leader is {} at {}. Our address is {}", leader, new_height, self.local_validator_addr);
            return Ok(());
        }

        // Are nodes requesting to create more than the minimum number of dummy blocks?
        let height_diff = high_qc.block_height().saturating_sub(new_height).as_u64();
        if height_diff > u64::try_from(epoch_state.local_committee().quorum_threshold()).unwrap_or(u64::MAX) {
            warn!(
                target: LOG_TARGET,
                "❌ Validator {from} sent NEWVIEW that attempts to create a larger than necessary number of dummy blocks. Expected requested {} < quorum threshold {}",
                height_diff,
                epoch_state.local_committee().quorum_threshold()
            );
            return Ok(());
        }

        let has_vote = last_vote.is_some();
        if let Some(vote) = last_vote {
            debug!(
                target: LOG_TARGET,
                "🔥 Receive VOTE with NEWVIEW for node {} {} from {}", vote.unverified_block_height, vote.block_id, from,
            );
            if let Err(err) = self
                .vote_collector
                .check_and_collect_vote(
                    from.clone(),
                    current_height,
                    epoch_state.epoch(),
                    vote,
                    epoch_state.local_committee_info(),
                )
                .await
            {
                warn!(target: LOG_TARGET, "❌ Error handling vote: {}", err);
                return Ok(());
            }
        }

        // Take note of unique NEWVIEWs so that we can count them
        let newview_count = self.collect_new_views(from, new_height, &high_qc);

        let latest_high_qc = self.store.with_write_tx(|tx| {
            high_qc.save(tx)?;
            high_qc.update_high_qc(tx)
        })?;

        let threshold = epoch_state.local_committee_info().quorum_threshold() as usize;

        info!(
            target: LOG_TARGET,
            "🌟 Received NEWVIEW (has_vote={}) (QUORUM: {}/{}) {} with high {}",
            has_vote,
            newview_count,
            threshold,
            new_height,
            latest_high_qc,
        );
        // Once we have received enough (quorum) NEWVIEWS, we can create the dummy block(s) and propose the next block.
        // Any subsequent NEWVIEWs for this height/view are ignored.
        if newview_count == threshold {
            info!(target: LOG_TARGET, "🌟✅ NEWVIEW height {} (high_qc: {}) has reached quorum ({}/{})", new_height, latest_high_qc, newview_count, threshold);

            self.pacemaker
                .update_view(epoch_state.epoch(), new_height, latest_high_qc.block_height())
                .await?;

            self.pacemaker.force_beat(new_height);
        }

        Ok(())
    }

    fn validate_qc(
        &self,
        qc: &QuorumCertificate,
        epoch_state: &EpochState<TConsensusSpec::Addr>,
        vote_signing_service: &TConsensusSpec::SignatureService,
    ) -> Result<(), ProposalValidationError> {
        if qc.epoch() != epoch_state.epoch() {
            return Err(ProposalValidationError::InvalidEpochInQc {
                block_id: *qc.block_id(),
                qc_id: *qc.id(),
                qc_epoch: qc.epoch(),
                current_epoch: epoch_state.epoch(),
            });
        }
        check_quorum_certificate_signatures::<TConsensusSpec>(qc, epoch_state.local_committee(), vote_signing_service)?;
        Ok(())
    }
}
