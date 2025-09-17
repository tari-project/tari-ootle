//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_bor::{Deserialize, Serialize};
use tari_engine_types::UtxoId;
use tari_ootle_common_types::{shard::Shard, StateVersion};
use tari_template_lib::types::crypto::{RistrettoPublicKeyBytes, UtxoTag};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoUpdateSet {
    pub shard_updates: HashMap<Shard, UtxoStateUpdateSet>,
    pub per_shard_high_watermark: Vec<(Shard, StateVersion)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoStateUpdateSet {
    pub updates: Vec<WalletUtxoUpdate>,
    pub max_state_version: StateVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum WalletUtxoUpdate {
    Unspent(UtxoUnspent),
    Spent(UtxoSpent),
    Burnt(UtxoBurnt),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoUnspent {
    pub tag: UtxoTag,
    pub public_nonce: RistrettoPublicKeyBytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoSpent {
    pub id: UtxoId,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoBurnt {
    pub id: UtxoId,
    pub version: u32,
}
