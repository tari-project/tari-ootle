//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{hash_map::Entry, BTreeMap, HashMap},
    sync::Arc,
};

use log::*;
use tari_common::configuration::Network;
use tari_common_types::types::FixedHash;
use tari_dan_common_types::{committee::CommitteeInfo, optional::Optional, Epoch, NodeHeight};
use tari_dan_storage::{
    consensus_models::{Block, BlockId, HighQc, QuorumCertificate, ValidatorSignature, Vote},
    global::models::ValidatorNode,
    StateStore,
};
use tari_epoch_manager::EpochManagerReader;
use tari_sidechain::QuorumDecision;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{
    hotstuff::error::HotStuffError,
    messages::VoteMessage,
    tracing::TraceTimer,
    traits::{ConsensusSpec, VoteSignatureService},
};

const LOG_TARGET: &str = "tari::dan::consensus::hotstuff::on_receive_vote";

#[derive(Clone)]
pub struct VoteCollector<TConsensusSpec: ConsensusSpec> {
    network: Network,
    vote_store: VoteStore,
    store: TConsensusSpec::StateStore,
    epoch_manager: TConsensusSpec::EpochManager,
    vote_signature_service: TConsensusSpec::SignatureService,
}

impl<TConsensusSpec> VoteCollector<TConsensusSpec>
where TConsensusSpec: ConsensusSpec
{
    pub fn new(
        network: Network,
        store: TConsensusSpec::StateStore,
        epoch_manager: TConsensusSpec::EpochManager,
        vote_signature_service: TConsensusSpec::SignatureService,
    ) -> Self {
        Self {
            network,
            store,
            vote_store: VoteStore::new(),
            epoch_manager,
            vote_signature_service,
        }
    }

    pub fn signing_service(&self) -> &TConsensusSpec::SignatureService {
        &self.vote_signature_service
    }

    /// Returns Some if quorum is reached
    pub async fn check_and_collect_vote(
        &self,
        from: TConsensusSpec::Addr,
        current_height: NodeHeight,
        current_epoch: Epoch,
        message: VoteMessage,
        local_committee_info: &CommitteeInfo,
    ) -> Result<Option<(QuorumCertificate, HighQc)>, HotStuffError> {
        let _timer = TraceTimer::debug(LOG_TARGET, "check_and_collect_vote");
        debug!(
            target: LOG_TARGET,
            "📬 Validating vote message from {from}: {message}"
        );

        {
            let mut store_mut = self.vote_store.get_mut().await;
            store_mut.clear_votes_before(current_epoch, current_height)?;
        }

        let sender_vn = self.check_eligibility(from, &message, local_committee_info).await?;
        self.validate_vote_message(current_epoch, &message)?;
        if self.collect_vote(&message, &sender_vn).await {
            warn!(
                target: LOG_TARGET,
                "❓️ Received duplicate vote for block {} from {}. This could be malicious because a validator should only vote once for the same block.",
                message.block_id,
                sender_vn.address
            );
            return Ok(None);
        }

        let quorum_threshold = local_committee_info.quorum_threshold() as usize;
        let threshold_decision = self
            .calculate_threshold_decision(&message.block_id, quorum_threshold)
            .await;

        let Some(quorum_decision) = threshold_decision.decision else {
            info!(
                target: LOG_TARGET,
                "🔥 Received vote for block {} {} {} from {} ({} of {}).",
                message.epoch,
                message.unverified_block_height,
                message.block_id,
                sender_vn.address,
                threshold_decision.count,
                local_committee_info.quorum_threshold()
            );
            return Ok(None);
        };

        // We only generate the next high qc once when we have a quorum of votes. Any votes received after this
        // are not included in the QC.
        if threshold_decision.count != quorum_threshold {
            info!(
                target: LOG_TARGET,
                "🔥 Received vote for block {} {} {} from {} ({} of {}).",
                message.epoch,
                message.unverified_block_height,
                message.block_id,
                sender_vn.address,
                threshold_decision.count,
                local_committee_info.quorum_threshold()
            );
            return Ok(None);
        }

        let new_qc = self
            .create_new_qc(&message.block_id, quorum_decision, local_committee_info)
            .await?;
        let high_qc = self.store.with_write_tx(|tx| new_qc.update_high_qc(tx))?;
        if new_qc.id() == high_qc.qc_id() {
            info!(target: LOG_TARGET, "🔥 New HIGH {}", new_qc);
        } else {
            info!(target: LOG_TARGET, "❓️ New QC from votes {} but it is not the high qc {}", new_qc, high_qc);
        }

        Ok(Some((new_qc, high_qc)))
    }

    async fn check_eligibility(
        &self,
        from: <TConsensusSpec as ConsensusSpec>::Addr,
        message: &VoteMessage,
        local_committee_info: &CommitteeInfo,
    ) -> Result<ValidatorNode<<TConsensusSpec as ConsensusSpec>::Addr>, HotStuffError> {
        // Does the vote come from a local committee member?
        let sender_vn = self
            .epoch_manager
            .get_validator_node_by_public_key(message.epoch, message.signature.public_key)
            .await
            .optional()?;
        let Some(sender_vn) = sender_vn else {
            return Err(HotStuffError::ReceivedVoteFromNonCommitteeMember {
                epoch: message.epoch,
                sender: from.to_string(),
                context: "VoteReceiver::handle_vote (sender pk not from registered VN)".to_string(),
            });
        };

        // Get the sender shard, and check that they are in the local committee
        if !local_committee_info.includes_substate_address(&sender_vn.shard_key) {
            return Err(HotStuffError::ReceivedVoteFromNonCommitteeMember {
                epoch: message.epoch,
                sender: sender_vn.address.to_string(),
                context: "VoteReceiver::handle_vote (VN not in local committee)".to_string(),
            });
        }

        Ok(sender_vn)
    }

    async fn collect_vote(
        &self,
        verified_message: &VoteMessage,
        sender_vn: &ValidatorNode<TConsensusSpec::Addr>,
    ) -> bool {
        let mut store_mut = self.vote_store.get_mut().await;
        let sender_leaf_hash = sender_vn.get_node_hash(self.network);
        store_mut.save_vote(Vote {
            epoch: verified_message.epoch,
            block_id: verified_message.block_id,
            // This has been verified
            block_height: verified_message.unverified_block_height,
            decision: verified_message.decision,
            sender_leaf_hash,
            signature: verified_message.signature.clone(),
        })
    }

    async fn create_new_qc(
        &self,
        block_id: &BlockId,
        quorum_decision: QuorumDecision,
        local_committee_info: &CommitteeInfo,
    ) -> Result<QuorumCertificate, HotStuffError> {
        let Some(block) = self.store.with_read_tx(|tx| Block::get(tx, block_id)).optional()? else {
            return Err(HotStuffError::InvariantError(format!(
                "Received votes for unknown block {}",
                block_id
            )));
        };

        let votes = self
            .vote_store
            .with_mut(|store_mut| store_mut.take_votes_with_decision_for_block(block_id, quorum_decision))
            .await;

        if votes.len() != local_committee_info.quorum_threshold() as usize {
            // This should be impossible without some bug, a panic wouldn't be inappropriate, however nodes could
            // recover from this bug, resuming consensus, after this crash.
            return Err(HotStuffError::InvariantError(format!(
                "create_new_qc: insufficient votes stored for block {}: {} < quorum {}",
                block_id,
                votes.len(),
                local_committee_info.quorum_threshold()
            )));
        }

        let signatures = votes.into_iter().map(|vote| vote.signature).collect();
        let new_qc = create_qc(signatures, quorum_decision, block);

        Ok(new_qc)
    }

    async fn calculate_threshold_decision(&self, block_id: &BlockId, quorum_threshold: usize) -> ThresholdDecision {
        let vote_store = self.vote_store.get_ref().await;
        let Some(votes_iter) = vote_store.votes_for_block_iter(block_id) else {
            // Soft invariant protection
            error!(
                target: LOG_TARGET,
                "INVARIANT: calculate_threshold_decision: no votes for block {}. Votes should have been collected before calling this function",
                block_id
            );
            // No votes yet - strange
            return ThresholdDecision {
                count: 0,
                decision: None,
            };
        };
        let mut count_accept = 0;
        let mut count_reject = 0;
        for vote in votes_iter {
            match vote.decision {
                QuorumDecision::Accept => count_accept += 1,
                QuorumDecision::Reject => count_reject += 1,
            }
        }

        if count_accept >= quorum_threshold {
            return ThresholdDecision {
                count: count_accept,
                decision: Some(QuorumDecision::Accept),
            };
        }
        if count_reject >= quorum_threshold {
            return ThresholdDecision {
                count: count_reject,
                decision: Some(QuorumDecision::Reject),
            };
        }

        ThresholdDecision {
            count: count_accept + count_reject,
            decision: None,
        }
    }

    fn validate_vote_message(&self, current_epoch: Epoch, message: &VoteMessage) -> Result<(), HotStuffError> {
        if current_epoch != message.epoch {
            return Err(HotStuffError::InvalidVote {
                signer_public_key: message.signature.public_key.to_string(),
                details: format!(
                    "Our current view is at epoch {} but the vote was for epoch {}",
                    current_epoch, message.epoch
                ),
            });
        }

        if !self
            .vote_signature_service
            .verify(&message.signature, &message.block_id, &message.decision)
        {
            return Err(HotStuffError::InvalidVoteSignature {
                signer_public_key: message.signature.public_key().to_string(),
            });
        }

        let maybe_known_block = self
            .store
            .with_read_tx(|tx| Block::get(tx, &message.block_id).optional())?;
        let Some(block) = maybe_known_block else {
            // This can happen if:
            // - The block is not yet in the store (race condition: the vote arrived before the block)
            // - The block ID is made up
            // warn!(
            //     target: LOG_TARGET,
            //     "❓️ Received vote for unknown block {}",  message.block_id,
            // );
            return Err(HotStuffError::InvalidVote {
                signer_public_key: message.signature.public_key.to_string(),
                details: format!(
                    "Received vote for unknown block {} (unverified height: {})",
                    message.block_id, message.unverified_block_height
                ),
            });
        };

        // TODO: necessary?
        if block.is_committed() || block.is_justified() {
            return Err(HotStuffError::InvalidVote {
                signer_public_key: message.signature.public_key.to_string(),
                details: format!(
                    "Received vote for block {} but it is already committed or justified",
                    message.block_id
                ),
            });
        }

        if block.epoch() != message.epoch {
            return Err(HotStuffError::InvalidVote {
                signer_public_key: message.signature.public_key.to_string(),
                details: format!(
                    "Received vote for block {} but it is not the current epoch {}",
                    message.block_id, message.epoch
                ),
            });
        }

        if block.height() != message.unverified_block_height {
            return Err(HotStuffError::InvalidVote {
                signer_public_key: message.signature.public_key.to_string(),
                details: format!(
                    "Received vote for block {} but it is not the current height {}",
                    message.block_id, message.unverified_block_height
                ),
            });
        }

        Ok(())
    }
}

fn create_qc(signatures: Vec<ValidatorSignature>, quorum_decision: QuorumDecision, block: Block) -> QuorumCertificate {
    QuorumCertificate::new(
        block.header().calculate_hash(),
        *block.parent(),
        block.height(),
        block.epoch(),
        block.shard_group(),
        signatures,
        quorum_decision,
    )
}

#[derive(Debug, Clone)]
struct VoteStore {
    store: Arc<RwLock<VoteStoreInner>>,
}

impl VoteStore {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(VoteStoreInner::new())),
        }
    }

    pub async fn get_ref(&self) -> RwLockReadGuard<'_, VoteStoreInner> {
        self.store.read().await
    }

    pub async fn get_mut(&self) -> RwLockWriteGuard<'_, VoteStoreInner> {
        self.store.write().await
    }

    pub async fn with_mut<F, R>(&self, f: F) -> R
    where F: FnOnce(&mut VoteStoreInner) -> R {
        let mut writer = self.get_mut().await;
        f(&mut writer)
    }
}

#[derive(Debug, Default)]
struct VoteStoreInner {
    /// Maps block IDs -> map (sender leaf hash -> vote)
    store: HashMap<BlockId, HashMap<FixedHash, Vote>>,
    blocks: BTreeMap<(Epoch, NodeHeight), BlockId>,
}

impl VoteStoreInner {
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
            blocks: BTreeMap::new(),
        }
    }

    pub fn clear_votes_before(&mut self, epoch: Epoch, height: NodeHeight) -> Result<(), HotStuffError> {
        let new_blocks = self.blocks.split_off(&(epoch, height.saturating_sub(NodeHeight(1))));
        self.log_buffer_size();
        for block_id in self.blocks.values() {
            self.store.remove(block_id);
        }
        self.blocks = new_blocks;
        self.shrink_map();
        Ok(())
    }

    /// Save a vote to the store. Returns true if a previous vote for the block by the sender was already present.
    /// Even if the vote is different, it is not considered a duplicate/invalid and does not overwrite the previous
    /// vote.
    pub fn save_vote(&mut self, vote: Vote) -> bool {
        // We use the sender leaf hash as the key to avoid collisions with other votes
        let key = &vote.sender_leaf_hash;
        match self.store.entry(vote.block_id) {
            Entry::Occupied(mut entry) => {
                if entry.get().contains_key(key) {
                    true
                } else {
                    self.blocks.insert((vote.epoch, vote.block_height), vote.block_id);
                    entry.get_mut().insert(*key, vote);
                    false
                }
            },
            Entry::Vacant(entry) => {
                let mut map = HashMap::new();
                self.blocks.insert((vote.epoch, vote.block_height), vote.block_id);
                map.insert(*key, vote);
                entry.insert(map);
                false
            },
        }
    }

    pub fn take_votes_with_decision_for_block(&mut self, block_id: &BlockId, decision: QuorumDecision) -> Vec<Vote> {
        let votes = self.store.remove(block_id).unwrap_or_default();
        self.shrink_map();
        votes.into_values().filter(|vote| vote.decision == decision).collect()
    }

    pub fn votes_for_block_iter(&self, block_id: &BlockId) -> Option<impl Iterator<Item = &Vote>> {
        let votes = self.store.get(block_id)?;
        Some(votes.values())
    }

    fn shrink_map(&mut self) {
        const MEM_WASTE_THRESHOLD: usize = 100;
        if self.store.capacity() - self.store.len() > MEM_WASTE_THRESHOLD {
            self.store.shrink_to_fit();
        }
    }

    fn log_buffer_size(&self) {
        const BLOCK_ID_SIZE: usize = size_of::<BlockId>();
        const VOTE_ENTRY_SIZE: usize = size_of::<Vote>() + size_of::<FixedHash>();
        debug!(
            target: LOG_TARGET,
            "Vote store size: used: {}KiB ({} entries), allocated: {} entries, blocks.len: {}",
            self.store.values().map(|v| BLOCK_ID_SIZE + v.len() * VOTE_ENTRY_SIZE).sum::<usize>() / 1024,
            self.store.len(),
            self.store.capacity(),
            self.blocks.len()
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct ThresholdDecision {
    pub decision: Option<QuorumDecision>,
    pub count: usize,
}
