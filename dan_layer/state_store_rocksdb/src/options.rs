//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[derive(Debug, Clone)]
pub struct DatabaseOptions {
    /// The versions behind the latest to keep for each shard.
    /// The default is 100. This preserves the last 100 versions of the state tree i.e. if the last 100 blocks all have
    /// state transitions, then we preserve 100 blocks worth of deleted state. Currently, this only applies to the
    /// state tree stale nodes.
    pub history_length: u64,
}

impl Default for DatabaseOptions {
    fn default() -> Self {
        Self { history_length: 10 }
    }
}
