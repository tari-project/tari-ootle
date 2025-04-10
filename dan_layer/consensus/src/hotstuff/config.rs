//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_common::configuration::Network;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::consensus_constants::ConsensusConstants;

#[derive(Debug, Clone)]
pub struct HotstuffConfig {
    pub network: Network,
    pub sidechain_id: Option<RistrettoPublicKeyBytes>,
    pub consensus_constants: ConsensusConstants,
    pub cleanup_interval: Duration,
}
