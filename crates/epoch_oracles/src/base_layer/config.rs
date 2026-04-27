//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

#[derive(Debug, Clone)]
pub struct BaseLayerEpochOracleConfig {
    /// The height to start querying from the base layer.
    pub start_height: u64,
    /// The number of blocks to lag behind the tip when querying data from the base layer (reorg mitigation)
    pub height_lag: u64,
    /// The interval between scans of the base layer for new epoch data
    pub scanning_interval: Duration,
    /// The sidechain ID to filter validator node changes by. If `None`, the "default" Ootle sidechain is used.
    pub sidechain_id: Option<RistrettoPublicKeyBytes>,
    /// Features for the base layer epoch oracle.
    pub features: BaseLayerEpochOracleFeatures,
    /// Number of base-layer blocks of leeway to allow when voting on `EndEpoch` proposals.
    /// If our scanner has not yet crossed the next epoch boundary, but our scanned (lagged)
    /// height is within this many blocks of the boundary, we accept EndEpoch proposals from
    /// peers whose oracle has already crossed. Set to 0 to disable leeway.
    pub epoch_end_spread_blocks: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct BaseLayerEpochOracleFeatures {
    /// Whether to sync headers when scanning the base layer.
    pub sync_headers: bool,
    /// Whether to sync validator node changes when scanning the base layer.
    pub sync_validator_node_changes: bool,
}
