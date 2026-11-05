//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::{Epoch, shard::Shard};
use tari_state_tree::Version;

use crate::{
    StateStoreReadTransaction,
    StorageError,
    consensus_models::{SubstateData, SubstateRecord, SubstateUpdateProof, SubstateValueOrHash},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateVersionTransitions {
    pub epoch: Epoch,
    pub shard: Shard,
    pub state_version: Version,
    pub updates: Vec<SubstateUpdateProof>,
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
        const CONFIDENTIAL_OUTPUT = 0x0000_0400;
        /// Populate `SubstateData.template_metadata` for template substates.
        /// On its own (without `TEMPLATE`), the response omits the WASM binary and carries
        /// `SubstateValueOrHash::Hash` for templates. When combined with `TEMPLATE`, the full
        /// binary is sent and the metadata field is populated alongside it.
        const TEMPLATE_METADATA = 0x0000_0200;
        /// Include every transition for substates outside the value filter: an up as its state hash
        /// alone, and a down in full (a down carries no value). This makes the stream a complete
        /// record of which substates changed, which a subscriber that caches substate values needs
        /// in order to tell a cached entry that is still current from one that has been superseded
        /// or destroyed.
        const ALL_HASHES = 0x1000_0000;
        /// Return the substates that are live at the point the stream is taken, rather than the
        /// transitions that produced them: ups whose substate is still live are sent, and ups whose
        /// substate has since been destroyed are skipped along with every down.
        ///
        /// Liveness must be decided by whether the substate is destroyed, never by whether it still
        /// holds a value. A destroyed substate keeps its value until epoch GC prunes it
        /// (`substates_prune_downed_values`), so a value-based test would report everything destroyed
        /// within the retention window as live, and no down follows to correct it.
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
            Self::CLAIMED_OUTPUT_TOMBSTONE |
            Self::CONFIDENTIAL_OUTPUT
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
            SubstateId::Template(_) => self.contains(Self::TEMPLATE) || self.contains(Self::TEMPLATE_METADATA),
            SubstateId::ValidatorFeePool(_) => self.contains(Self::VALIDATOR_FEE_POOL),
            SubstateId::Utxo(_) => self.contains(Self::UTXO),
            SubstateId::ConfidentialOutput(_) => self.contains(Self::CONFIDENTIAL_OUTPUT),
            SubstateId::ClaimedOutputTombstone(_) => self.contains(Self::CLAIMED_OUTPUT_TOMBSTONE),
        }
    }

    /// `substate` as these filters serve it, or `None` if they serve it in neither form.
    ///
    /// A substate the filters select carries its value while it still holds one, and its state hash once its value is
    /// pruned. A template carries its metadata under `TEMPLATE_METADATA`, and its value only under `TEMPLATE` too. A
    /// substate the filters do not select carries its state hash under `ALL_HASHES`.
    pub fn serve(&self, substate: SubstateRecord) -> Option<SubstateData> {
        let metadata_mode = self.contains(Self::TEMPLATE_METADATA);
        let template_metadata = if metadata_mode && substate.substate_id.is_template() {
            substate
                .substate_value
                .as_ref()
                .and_then(|v| v.as_template())
                .map(|t| t.clone().into_template_metadata())
        } else {
            None
        };
        let is_metadata_only = metadata_mode && !self.contains(Self::TEMPLATE) && substate.substate_id.is_template();
        let value = if self.contains_substate(&substate.substate_id) {
            substate
                .substate_value
                .filter(|_| !is_metadata_only)
                .map(|v| SubstateValueOrHash::Value(Box::new(v)))
                .unwrap_or(SubstateValueOrHash::Hash(substate.state_hash))
        } else if self.include_filtered_hashes() {
            SubstateValueOrHash::Hash(substate.state_hash)
        } else {
            return None;
        };
        Some(SubstateData {
            substate_id: substate.substate_id,
            version: substate.version,
            value,
            template_metadata,
        })
    }
}
