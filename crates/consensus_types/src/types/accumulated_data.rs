//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::BorshSerialize;
use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, BorshSerialize, Encode, Decode, CborLen)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ShardGroupAccumulatedData {
    // minicbor 2.2 has no Encode/Decode for u128; bridge via tari_bor's u128 adapter until
    // upstream PR https://github.com/twittner/minicbor/pull/63 lands.
    #[n(0)]
    #[cbor(with = "tari_bor::adapters::u128_codec")]
    pub total_exhaust_burn: u128,
}

impl From<ShardGroupAccumulatedData> for tari_sidechain::ShardGroupAccumulatedData {
    fn from(value: ShardGroupAccumulatedData) -> Self {
        Self {
            total_exhaust_burn: value.total_exhaust_burn,
        }
    }
}
