//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt::Display, time::Duration};

#[derive(Debug, Clone, Default)]
pub struct StateSyncStats {
    pub total_transitions: u64,
    pub total_time: Duration,
    pub total_requests: u64,
}

impl Display for StateSyncStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Sync stats: {{ total_transitions: {}, total_time: {:.2?}, total_requests: {} }}",
            self.total_transitions, self.total_time, self.total_requests
        )
    }
}
