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
}
