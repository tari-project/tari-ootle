//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::num::NonZeroU32;

use tari_dan_common_types::NumPreshards;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

#[derive(Debug, Clone)]
pub struct EpochManagerConfig {
    pub base_layer_confirmations: u64,
    pub committee_size: NonZeroU32,
    pub validator_node_sidechain_id: Option<RistrettoPublicKeyBytes>,
    pub num_preshards: NumPreshards,
    pub fee_claim_public_key: RistrettoPublicKeyBytes,
}
