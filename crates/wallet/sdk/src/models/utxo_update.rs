//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use tari_bor::{Deserialize, Serialize};
use tari_ootle_common_types::{Epoch, StateVersion, shard::Shard};
use tari_template_lib::types::{
    UtxoId,
    crypto::{RistrettoPublicKeyBytes, UtxoTag},
};

#[derive(Debug, Clone)]
pub struct UtxoUpdatePayload {
    pub sos: Option<StartOfShard>,
    pub update: Option<WalletUtxoUpdate>,
    pub eos: Option<EndOfShard>,
}

#[derive(Debug, Clone)]
pub struct StartOfShard {
    pub shard: Shard,
    pub max_state_version: StateVersion,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
pub struct EndOfShard {
    pub max_state_version: StateVersion,
}

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
    pub max_epoch: Epoch,
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
