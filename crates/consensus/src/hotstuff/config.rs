//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use tari_ootle_common_types::Network;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::consensus_constants::ConsensusConstants;

#[derive(Debug, Clone)]
pub struct HotstuffConfig {
    pub network: Network,
    pub sidechain_id: Option<RistrettoPublicKeyBytes>,
    pub consensus_constants: ConsensusConstants,
    pub state_tree_cleanup_interval: Duration,
    pub epoch_gc_interval: Duration,
    pub enable_eviction_proposal: bool,
    pub epoch_end_grace_period: Duration,
    /// Maximum number of concurrent foreign proposal request tasks
    pub max_foreign_proposal_tasks: usize,
}
