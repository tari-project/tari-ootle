//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub use tari_indexer_client::types::{
    UtxoBurnt,
    UtxoSpent,
    UtxoStateUpdateSet,
    UtxoUnspent,
    UtxoUpdateSet,
    WalletUtxoUpdate,
};
use tari_ootle_common_types::{StateVersion, shard::Shard};

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
