//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::PublicKey;
use tari_dan_common_types::Epoch;

use crate::{consensus_models::BlockId, StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};

#[derive(Debug, Clone, Copy)]
pub struct ValidatorStatsUpdate<'a> {
    public_key: &'a PublicKey,

    /// None = no change, Some(n) = inc failure by n, Some(0) = clear failures
    missed_proposal_change: Option<i64>,
    participation_shares_increment: u64,
    max_missed_proposal_count: u64,
}

impl<'a> ValidatorStatsUpdate<'a> {
    pub fn new(public_key: &'a PublicKey) -> Self {
        Self {
            public_key,
            missed_proposal_change: None,
            participation_shares_increment: 0,
            max_missed_proposal_count: 5,
        }
    }

    pub fn public_key(&self) -> &PublicKey {
        self.public_key
    }

    pub fn missed_proposal_change(&self) -> Option<i64> {
        self.missed_proposal_change
    }

    pub fn participation_shares_increment(&self) -> u64 {
        self.participation_shares_increment
    }

    pub fn add_missed_proposal(mut self) -> Self {
        self.missed_proposal_change = Some(1);
        self
    }

    pub fn decrement_missed_proposal(mut self) -> Self {
        self.missed_proposal_change = Some(-1);
        self
    }

    /// Sets a cap for the missed proposal count.
    pub fn set_max_missed_proposals_cap(mut self, n: u64) -> Self {
        self.max_missed_proposal_count = n;
        self
    }

    pub fn max_total_missed_proposals(&self) -> i64 {
        i64::try_from(self.max_missed_proposal_count).unwrap_or(i64::MAX)
    }

    pub fn reset_missed_proposals(mut self) -> Self {
        self.missed_proposal_change = Some(0);
        self
    }

    pub fn increment_participation_share(mut self) -> Self {
        self.participation_shares_increment = 1;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ValidatorConsensusStats {
    pub missed_proposals: u64,
    pub participation_shares: u64,
}

impl ValidatorConsensusStats {
    pub fn get_nodes_to_evict<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        block_id: &BlockId,
        threshold: u64,
        limit: u64,
    ) -> Result<Vec<PublicKey>, StorageError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        tx.validator_epoch_stats_get_nodes_to_evict(block_id, threshold, limit)
    }

    pub fn get_by_public_key<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        epoch: Epoch,
        public_key: &PublicKey,
    ) -> Result<Self, StorageError> {
        tx.validator_epoch_stats_get(epoch, public_key)
    }

    pub fn is_node_evicted<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        block_id: &BlockId,
        public_key: &PublicKey,
    ) -> Result<bool, StorageError> {
        tx.suspended_nodes_is_evicted(block_id, public_key)
    }

    pub fn evict_node<TTx: StateStoreWriteTransaction>(
        tx: &mut TTx,
        public_key: &PublicKey,
        evicted_in_block: BlockId,
    ) -> Result<(), StorageError> {
        tx.evicted_nodes_evict(public_key, evicted_in_block)
    }

    pub fn count_number_evicted_nodes<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        epoch: Epoch,
    ) -> Result<u64, StorageError> {
        tx.evicted_nodes_count(epoch)
    }
}
