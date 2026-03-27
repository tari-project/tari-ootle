//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::num::NonZeroUsize;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{Epoch, shard::Shard};
use tari_state_tree::Version;

use crate::{StateStoreReadTransaction, StorageError, consensus_models::SubstateUpdateProof};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateVersionTransitions {
    pub epoch: Epoch,
    pub shard: Shard,
    pub state_version: Version,
    pub updates: Vec<SubstateUpdateProof>,
}

impl StateVersionTransitions {
    pub fn into_chunks(self, size: NonZeroUsize) -> Vec<Self> {
        let num_chunks = self.updates.len().div_ceil(size.get());
        let mut chunks = Vec::with_capacity(num_chunks);
        let mut updates = self.updates;
        while !updates.is_empty() {
            let take = updates.len().min(size.get());
            let chunk_updates: Vec<_> = updates.drain(..take).collect();
            chunks.push(Self {
                epoch: self.epoch,
                shard: self.shard,
                state_version: self.state_version,
                updates: chunk_updates,
            });
        }
        chunks
    }
}

pub struct StateTransition;

impl StateTransition {
    pub fn get_for_shard<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        shard: Shard,
        state_version: Version,
        value_filters: SubstateValueFilterFlags,
    ) -> Result<StateVersionTransitions, StorageError> {
        tx.state_transitions_get_starting_at(shard, state_version, value_filters)
    }
}

bitflags! {
    /// Represents a set of flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct SubstateValueFilterFlags: u32 {
        const NONE = 0;
        const COMPONENT = 0x0000_0001;
        const RESOURCE = 0x0000_0002;
        const VAULT = 0x0000_0004;
        const NON_FUNGIBLE = 0x0000_0008;
        const TRANSACTION_RECEIPT = 0x0000_0010;
        const TEMPLATE = 0x0000_0020;
        const VALIDATOR_FEE_POOL = 0x0000_0040;
        const UTXO = 0x0000_0080;
        const CLAIMED_OUTPUT_TOMBSTONE = 0x0000_0100;
        /// Include all hashes for substates even if filtered out
        const ALL_HASHES = 0x1000_0000;
        const UP_ONLY = 0x0100_0000;
    }
}

impl SubstateValueFilterFlags {
    pub fn all_substates() -> Self {
        Self::COMPONENT |
            Self::RESOURCE |
            Self::VAULT |
            Self::NON_FUNGIBLE |
            Self::TRANSACTION_RECEIPT |
            Self::TEMPLATE |
            Self::VALIDATOR_FEE_POOL |
            Self::UTXO |
            Self::CLAIMED_OUTPUT_TOMBSTONE
    }

    pub fn include_filtered_hashes(&self) -> bool {
        self.contains(Self::ALL_HASHES)
    }

    pub fn is_up_only(&self) -> bool {
        self.contains(Self::UP_ONLY)
    }

    pub fn contains_substate(&self, substate_id: &SubstateId) -> bool {
        match substate_id {
            SubstateId::Component(_) => self.contains(Self::COMPONENT),
            SubstateId::Resource(_) => self.contains(Self::RESOURCE),
            SubstateId::Vault(_) => self.contains(Self::VAULT),
            SubstateId::NonFungible(_) => self.contains(Self::NON_FUNGIBLE),
            SubstateId::TransactionReceipt(_) => self.contains(Self::TRANSACTION_RECEIPT),
            SubstateId::Template(_) => self.contains(Self::TEMPLATE),
            SubstateId::ValidatorFeePool(_) => self.contains(Self::VALIDATOR_FEE_POOL),
            SubstateId::Utxo(_) => self.contains(Self::UTXO),
            SubstateId::ClaimedOutputTombstone(_) => self.contains(Self::CLAIMED_OUTPUT_TOMBSTONE),
        }
    }
}
