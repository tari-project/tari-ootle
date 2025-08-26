//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_bor::{Deserialize, Serialize};
use tari_engine_types::{Utxo, UtxoAddress};

use crate::{shard::Shard, StateVersion};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum UtxoUpdate {
    Unspent(UtxoUnspent),
    Spent(UtxoSpent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoUnspent {
    pub address: UtxoAddress,
    pub version: u32,
    pub shard: Shard,
    pub state_version: StateVersion,
    pub utxo: Utxo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoSpent {
    pub address: UtxoAddress,
    pub version: u32,
    pub shard: Shard,
    pub state_version: StateVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UtxoQueryResponse {
    pub updates: Vec<UtxoUpdate>,
    pub per_shard_high_watermark: Vec<(Shard, StateVersion)>,
}
