//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, sync::Arc, time::Duration};

use tari_template_lib_types::TemplateAddress;

use crate::network_state_sync::EventFilter;

#[derive(Debug, Clone)]
pub struct NetworkWideStateSyncConfig {
    pub work_interval: Duration,
    pub event_filters: Arc<[EventFilter]>,
    pub watched_templates: Arc<HashSet<TemplateAddress>>,
}

impl Default for NetworkWideStateSyncConfig {
    fn default() -> Self {
        Self {
            work_interval: Duration::from_secs(30),
            event_filters: Arc::new([]),
            watched_templates: Arc::new(HashSet::new()),
        }
    }
}
