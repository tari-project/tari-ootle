//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::{Epoch, ShardGroup};

#[derive(Debug, Clone)]
pub enum EpochManagerEvent {
    EpochChanged {
        epoch: Epoch,
        /// Some if the local validator is registered for the epoch, otherwise None
        registered_shard_group: Option<ShardGroup>,
    },
}
