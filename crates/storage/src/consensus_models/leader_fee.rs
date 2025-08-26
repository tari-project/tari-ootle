//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::{Display, Formatter},
    num::NonZeroU64,
};

use borsh::BorshSerialize;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LeaderFee {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub fee: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub global_exhaust_burn: u64,
}

impl LeaderFee {
    pub fn fee(&self) -> u64 {
        self.fee
    }

    pub fn global_exhaust_burn(&self) -> u64 {
        self.global_exhaust_burn
    }
}

impl Display for LeaderFee {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Leader fee: {}, Burnt: {}", self.fee, self.global_exhaust_burn)
    }
}

pub fn calculate_leader_fee(transaction_fee: u64, num_involved_shards: NonZeroU64, exhaust_divisor: u64) -> LeaderFee {
    let target_burn = transaction_fee.checked_div(exhaust_divisor).unwrap_or(0);
    let block_fee_after_burn = transaction_fee - target_burn;

    let mut leader_fee = block_fee_after_burn / num_involved_shards;
    // The extra amount that is burnt from dividing the number of shards involved
    let excess_remainder_burn = block_fee_after_burn % num_involved_shards;

    // Adjust the leader fee to account for the remainder
    // If the remainder accounts for an extra burn of greater than half the number of involved shards, we
    // give each validator an extra 1 in fees if enough fees are available, burning less than the exhaust target.
    // Otherwise, we burn a little more than/equal to the exhaust target.
    let actual_burn = if excess_remainder_burn > 0 &&
        // If the div floor burn accounts for 1 less fee for more than half of number of shards, and ...
        excess_remainder_burn >= num_involved_shards.get() / 2 &&
        // ... if there are enough fees to pay out an additional 1 to all shards
        (leader_fee + 1) * num_involved_shards.get() <= transaction_fee
    {
        // Pay each leader 1 more
        leader_fee += 1;

        // We burn a little less due to the remainder
        target_burn.saturating_sub(num_involved_shards.get() - excess_remainder_burn)
    } else {
        // We burn a little more due to the remainder
        target_burn + excess_remainder_burn
    };

    LeaderFee {
        fee: leader_fee,
        global_exhaust_burn: actual_burn,
    }
}
