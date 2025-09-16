//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{sync::Arc, time::Duration};

use crate::network_state_sync::EventFilter;

#[derive(Debug, Clone)]
pub struct NetworkWideStateSyncConfig {
    pub work_interval: Duration,
    pub event_filters: Arc<[EventFilter]>,
}

impl Default for NetworkWideStateSyncConfig {
    fn default() -> Self {
        Self {
            work_interval: Duration::from_secs(60),
            event_filters: Arc::new([]),
        }
    }
}
