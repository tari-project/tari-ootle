//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_common::configuration::Network;
use tari_consensus_types::{HighPc, ProposalCertificate, ProposalVote, ValidatorSignatureBytes};
use tari_dan_common_types::{committee::CommitteeInfo, optional::Optional, Epoch, NodeHeight};
use tari_dan_storage::{consensus_models::Block, StateStore};
use tari_sidechain::QuorumDecision;

use super::collector::VoteCollector;
use crate::{
    hotstuff::{error::HotStuffError, vote_collector::helpers::check_eligibility},
    tracing::TraceTimer,
    traits::{CertificateStore, ConsensusSpec, ValidatorSignatureVerifierService},
};

const LOG_TARGET: &str = "tari::consensus::hotstuff::proposal_collector";

#[derive(Clone)]
pub struct ProposalVoteCollector<TConsensusSpec: ConsensusSpec> {
    network: Network,
    vote_collector: VoteCollector<ProposalVote>,
    store: TConsensusSpec::StateStore,
    epoch_manager: TConsensusSpec::EpochManager,
    vote_signer_service: TConsensusSpec::SignerService,
}

impl<TConsensusSpec> ProposalVoteCollector<TConsensusSpec>
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

    pub fn signing_service(&self) -> &TConsensusSpec::SignerService {
        &self.vote_signer_service
    }

    /// Returns Some if quorum is reached
    pub async fn check_and_collect_vote(
        &self,
        from: TConsensusSpec::Addr,
        current_height: NodeHeight,
        current_epoch: Epoch,
        vote: ProposalVote,
        local_committee_info: &CommitteeInfo,
    ) -> Result<Option<(ProposalCertificate, HighPc)>, HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "check_and_collect_vote");
        debug!(
            target: LOG_TARGET,
            "Validating vote message from {from}: {vote}"
        );

        let block_id = vote.block_id;
        let sender_vn =
            check_eligibility::<TConsensusSpec, _>(&self.epoch_manager, from, &vote, local_committee_info).await?;
        self.validate_vote(current_epoch, &vote)?;
        let quorum_threshold = local_committee_info.quorum_threshold() as usize;
        let sender_leaf_hash = sender_vn.get_node_hash(self.network);
        let Some((quorum_votes, quorum_decision)) = self
            .vote_collector
            .collect_vote(current_epoch, current_height, sender_leaf_hash, vote, quorum_threshold)
            .await
        else {
            return Ok(None);
        };

        let Some(block) = self.store.with_read_tx(|tx| Block::get(tx, &block_id).optional())? else {
            warn!(
                target: LOG_TARGET,
                "❓️ Received vote for unknown block {}. Possible race condition where a quorum of votes arrived before the block.",
                block_id
            );
            return Ok(None);
        };
        let signatures = quorum_votes.into_iter().map(|vote| vote.signature).collect();
        let new_qc = create_proposal_certificate(signatures, quorum_decision, block);
        let high_qc = self.store.with_write_tx(|tx| new_qc.update_highest(tx))?;
        if new_qc.calculate_id() == *high_qc.id() {
            info!(target: LOG_TARGET, "🔥 New HIGH {}", new_qc);
        } else {
            info!(target: LOG_TARGET, "❓️ New QC from votes {} but it is not the high qc {}", new_qc, high_qc);
        }

        Ok(Some((new_qc, high_qc)))
    }

    fn validate_vote(&self, current_epoch: Epoch, vote: &ProposalVote) -> Result<(), HotStuffError> {
        if current_epoch != vote.epoch {
            return Err(HotStuffError::InvalidVote {
                signer_public_key: vote.signature.public_key,
                details: format!(
                    "Our current view is at epoch {} but the vote was for epoch {}",
                    current_epoch, vote.epoch
                ),
            });
        }

        if !self.vote_signer_service.verify(vote) {
            return Err(HotStuffError::InvalidVoteSignature {
                signer_public_key: *vote.signature.public_key(),
            });
        }

        Ok(())
    }
}

fn create_proposal_certificate(
    signatures: Vec<ValidatorSignatureBytes>,
    quorum_decision: QuorumDecision,
    block: Block,
) -> ProposalCertificate {
    ProposalCertificate::new(
        block.header().calculate_hash(),
        *block.parent(),
        block.height(),
        block.epoch(),
        block.shard_group(),
        signatures,
        quorum_decision,
    )
}
