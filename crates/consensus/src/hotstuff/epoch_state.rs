//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::types::FixedHash;
use tari_epoch_manager::{EpochManagerError, EpochManagerReader};
use tari_ootle_common_types::{
    Epoch,
    committee::{Committee, CommitteeInfo},
};

pub struct EpochState<TAddr> {
    /// The current epoch
    pub epoch: Epoch,
    pub epoch_hash: FixedHash,
    // /// The shard group for the local validator in the current epoch
    // pub registered_shard_group: Option<ShardGroup>,
    pub local_committee_info: CommitteeInfo,
    pub local_committee: Committee<TAddr>,
}

impl<TAddr> EpochState<TAddr> {
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub fn epoch_hash(&self) -> &FixedHash {
        &self.epoch_hash
    }

    pub fn local_committee_info(&self) -> &CommitteeInfo {
        &self.local_committee_info
    }

    pub fn local_committee(&self) -> &Committee<TAddr> {
        &self.local_committee
    }

    pub async fn update_from_epoch_manager<TEpochManager: EpochManagerReader<Addr = TAddr>>(
        &mut self,
        epoch_manager: &TEpochManager,
        current_epoch: Epoch,
    ) -> Result<(), EpochManagerError> {
        self.local_committee_info = epoch_manager.get_local_committee_info(current_epoch).await?;
        self.local_committee = epoch_manager.get_local_committee(current_epoch).await?;
        self.epoch_hash = epoch_manager.get_current_epoch_hash().await?;
        self.epoch = current_epoch;

        Ok(())
    }
}
