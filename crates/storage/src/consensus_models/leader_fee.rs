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
    pub fee: u64,
    pub exhaust_burn: u64,
}

impl LeaderFee {
    pub fn fee(&self) -> u64 {
        self.fee
    }

    pub fn exhaust_burn(&self) -> u64 {
        self.exhaust_burn
    }
}

impl Display for LeaderFee {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Leader fee: {}, Burnt: {}", self.fee, self.exhaust_burn)
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

        // We burn a little less (< num_involved_shards) due to the remainder
        target_burn.saturating_sub(num_involved_shards.get() - excess_remainder_burn)
    } else {
        // We burn a little more (< num_involved_shards) due to the remainder
        target_burn + excess_remainder_burn
    };

    LeaderFee {
        fee: leader_fee,
        exhaust_burn: actual_burn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_calculates_the_correct_leader_fee_and_burn() {
        let test_cases = [
            // (transaction_fee, num_involved_shards, exhaust_divisor, expected_leader_fee, expected_burn)
            // 10% burn target
            (100, 1, 10, 90, 10),
            (100, 2, 10, 45, 10),
            (100, 3, 10, 30, 10),
            (100, 4, 10, 23, 8),
            (100, 5, 10, 18, 10),
            (100, 6, 10, 15, 10),
            (100, 7, 10, 13, 9),
            (100, 8, 10, 11, 12),
            (100, 9, 10, 10, 10),
            (100, 10, 10, 9, 10),
            // 20% burn target
            (100, 1, 5, 80, 20),
            (100, 2, 5, 40, 20),
            (100, 3, 5, 27, 19),
            (100, 4, 5, 20, 20),
            (100, 5, 5, 16, 20),
            (100, 6, 5, 13, 22),
            (100, 7, 5, 12, 16),
            (100, 8, 5, 10, 20),
            (100, 9, 5, 9, 19),
            (100, 10, 5, 8, 20),
            // 20% burn target
            (55, 3, 5, 15, 10),
            (55, 4, 5, 11, 11),
            (55, 5, 5, 9, 10),
            (55, 6, 5, 7, 13),
            (55, 7, 5, 6, 13),
            (55, 8, 5, 6, 7),
            (55, 9, 5, 5, 10),
            (55, 10, 5, 4, 15),
        ];

        for (transaction_fee, num_involved_shards, exhaust_divisor, expected_leader_fee, expected_burn) in test_cases {
            let num_involved_shards = NonZeroU64::new(num_involved_shards).unwrap();
            let leader_fee = calculate_leader_fee(transaction_fee as u64, num_involved_shards, exhaust_divisor as u64);
            assert_eq!(
                leader_fee.fee * num_involved_shards.get() + leader_fee.exhaust_burn,
                transaction_fee as u64,
                "In/deflation! transaction_fee: {transaction_fee}, num_involved_shards: {num_involved_shards}, \
                 exhaust_divisor: {exhaust_divisor}",
            );
            assert_eq!(
                leader_fee.fee(),
                expected_leader_fee as u64,
                "Failed for transaction_fee: {}, num_involved_shards: {}, exhaust_divisor: {}",
                transaction_fee,
                num_involved_shards,
                exhaust_divisor
            );
            assert_eq!(
                leader_fee.exhaust_burn(),
                expected_burn as u64,
                "Failed for transaction_fee: {}, num_involved_shards: {}, exhaust_divisor: {}",
                transaction_fee,
                num_involved_shards,
                exhaust_divisor
            );
        }
    }
}
