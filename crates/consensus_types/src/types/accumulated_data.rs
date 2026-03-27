//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ShardGroupAccumulatedData {
    pub total_exhaust_burn: u128,
}

impl From<ShardGroupAccumulatedData> for tari_sidechain::ShardGroupAccumulatedData {
    fn from(value: ShardGroupAccumulatedData) -> Self {
        Self {
            total_exhaust_burn: value.total_exhaust_burn,
        }
    }
}
