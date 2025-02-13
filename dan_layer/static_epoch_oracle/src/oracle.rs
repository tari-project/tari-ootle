//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle};

use crate::config::Config;

pub struct StaticEpochOracle {
    _epoch_time: Duration,
    _config: Config,
}

impl EpochEventOracle for StaticEpochOracle {
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        None
    }
}
