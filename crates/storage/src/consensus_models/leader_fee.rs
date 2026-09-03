//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::{Display, Formatter},
    num::NonZeroU64,
};

use borsh::BorshSerialize;
use minicbor::{CborLen, Decode, Encode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, Encode, Decode, CborLen)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LeaderFee {
    /// The fee payable to the leader of each involved shard group.
    #[n(0)]
    pub fee: u64,
    /// The amount burned for the whole transaction across all involved shard groups: the exhaust burn collected by
    /// the executor plus the indivisible remainder from dividing the transaction fee between the leaders
    /// (`fee * num_involved_shard_groups + exhaust_burn == transaction_fee + executor_exhaust_burn`).
    ///
    /// CONSENSUS RULE: this must equal the amount actually withheld from validators, exactly — the accumulated burn
    /// in block headers determines the total supply, so it must not be re-derived with lossy arithmetic. Each shard
    /// group accumulates only its portion of this into its block header burn total — see
    /// `Evidence::exhaust_burn_portion`.
    #[n(1)]
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

/// Calculates the fee payable to the leader of each involved shard group and the whole-transaction burn.
///
/// `transaction_fee` is the leader's share of what the transaction paid, divided evenly across the involved shard
/// groups.
/// `exhaust_burn` is the burn the executor collected for this transaction (`FeeReceipt::exhaust_burn`);
/// the indivisible remainder of the fee division is added to it, so no amount is created or lost:
/// `fee * num_involved_shards + exhaust_burn == transaction_fee + executor_exhaust_burn`, exactly.
pub fn calculate_leader_fee(transaction_fee: u64, exhaust_burn: u64, num_involved_shards: NonZeroU64) -> LeaderFee {
    LeaderFee {
        fee: transaction_fee / num_involved_shards,
        exhaust_burn: exhaust_burn + transaction_fee % num_involved_shards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_calculates_the_correct_leader_fee_and_burn() {
        let test_cases = [
            // (transaction_fee, executor_burn, num_involved_shards, expected_leader_fee, expected_burn)
            (100, 5, 1, 100, 5),
            (100, 5, 2, 50, 5),
            (100, 5, 3, 33, 6),
            (100, 5, 4, 25, 5),
            (100, 5, 6, 16, 9),
            (100, 5, 7, 14, 7),
            (100, 5, 10, 10, 5),
            (105, 5, 2, 52, 6),
            (55, 2, 3, 18, 3),
            (55, 2, 7, 7, 8),
            // no executor burn: only the leader-fee division remainder is burned
            (101, 0, 2, 50, 1),
            (100, 0, 1, 100, 0),
            // zero-fee transaction still burns whatever the executor collected
            (0, 3, 2, 0, 3),
        ];

        for (transaction_fee, executor_burn, num_involved_shards, expected_leader_fee, expected_burn) in test_cases {
            let num_involved_shards = NonZeroU64::new(num_involved_shards).unwrap();
            let leader_fee = calculate_leader_fee(transaction_fee, executor_burn, num_involved_shards);
            assert_eq!(
                leader_fee.fee * num_involved_shards.get() + leader_fee.exhaust_burn,
                transaction_fee + executor_burn,
                "In/deflation! transaction_fee: {transaction_fee}, executor_burn: {executor_burn}, \
                 num_involved_shards: {num_involved_shards}",
            );
            assert_eq!(
                leader_fee.fee(),
                expected_leader_fee,
                "Failed for transaction_fee: {transaction_fee}, executor_burn: {executor_burn}, num_involved_shards: \
                 {num_involved_shards}",
            );
            assert_eq!(
                leader_fee.exhaust_burn(),
                expected_burn,
                "Failed for transaction_fee: {transaction_fee}, executor_burn: {executor_burn}, num_involved_shards: \
                 {num_involved_shards}",
            );
        }
    }
}
