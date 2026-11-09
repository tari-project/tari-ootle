//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::time::Instant;

use tari_ootle_common_types::{Epoch, ShardGroup};

#[derive(Debug, Clone)]
pub enum EpochManagerEvent {
    EpochChanged {
        epoch: Epoch,
        /// Some if the local validator is registered for the epoch, otherwise None
        registered_shard_group: Option<ShardGroup>,
        activated_at: Instant,
    },
    /// The local validator has a registration that activates at `activation_epoch`, later than the current epoch.
    /// [`EpochManagerReader::get_pending_activation`](crate::EpochManagerReader::get_pending_activation) returns the
    /// shard group to sync ahead of it.
    PendingActivation { activation_epoch: Epoch },
}

/// A registration of the local validator that activates after the current epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingActivation {
    pub activation_epoch: Epoch,
    /// The shard group the validator's shard key falls in under the current epoch's committees. With a single shard
    /// group this is the group it joins at activation.
    pub shard_group: ShardGroup,
}
