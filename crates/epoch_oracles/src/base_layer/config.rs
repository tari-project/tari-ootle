//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_template_lib::prelude::RistrettoPublicKeyBytes;

pub struct BaseLayerEpochOracleConfig {
    /// The height to start querying from the base layer.
    pub start_height: u64,
    /// The number of blocks to lag behind the tip when querying data from the base layer (reorg mitigation)
    pub height_lag: u64,
    /// The interval between scans of the base layer for new epoch data
    pub scanning_interval: Duration,
    /// The sidechain ID to filter validator node changes by. If `None`, the "default" Ootle sidechain is used.
    pub sidechain_id: Option<RistrettoPublicKeyBytes>,
    /// Whether to sync headers when scanning the base layer.
    pub sync_headers: bool,
}
