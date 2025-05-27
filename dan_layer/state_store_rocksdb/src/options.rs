//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_dan_common_types::Epoch;

#[derive(Debug, Clone)]
pub struct DatabaseOptions {
    /// The versions behind the latest to keep for each shard.
    /// The default is 100. This preserves the last 100 versions of the state tree i.e. if the last 100 blocks all have
    /// state transitions, then we preserve 100 blocks worth of deleted state. Currently, this only applies to the
    /// state tree stale nodes.
    pub state_history_length: u64,
    /// The number of epochs back from the current epoch to keep in the database.
    /// This includes blocks, foreign proposals etc.
    /// The default is 1, which means we keep the previous epoch's data. It is not recommended to set this to 0.
    pub epoch_history_length: Epoch,
}

impl Default for DatabaseOptions {
    fn default() -> Self {
        Self {
            state_history_length: 100,
            epoch_history_length: Epoch(1),
        }
    }
}
