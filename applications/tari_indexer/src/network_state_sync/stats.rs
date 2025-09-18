//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    pub total_checkpoints: usize,
    pub total_state_updates: usize,
    pub total_events: usize,
}

impl SyncStats {
    pub fn new() -> Self {
        Self {
            total_checkpoints: 0,
            total_state_updates: 0,
            total_events: 0,
        }
    }

    pub fn increment_checkpoints(&mut self) {
        self.total_checkpoints += 1;
    }

    pub fn increase_state_updates(&mut self, by: usize) {
        self.total_state_updates += by;
    }

    pub fn increase_events(&mut self, by: usize) {
        self.total_events += by;
    }

    pub fn log_stats(&self) {
        log::info!("{}", self);
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Display for SyncStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Sync Stats: {{ checkpoints: {}, state_updates: {}, events: {} }}",
            self.total_checkpoints, self.total_state_updates, self.total_events
        )
    }
}
