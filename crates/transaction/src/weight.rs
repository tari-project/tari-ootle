//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{iter::Sum, ops::Add};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TransactionWeight(u64);

impl TransactionWeight {
    pub fn new(weight: u64) -> Self {
        Self(weight)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Sum<u64> for TransactionWeight {
    fn sum<I: Iterator<Item = u64>>(iter: I) -> Self {
        Self(iter.fold(0u64, u64::saturating_add))
    }
}

impl Add for TransactionWeight {
    type Output = TransactionWeight;

    fn add(self, rhs: Self) -> Self::Output {
        TransactionWeight(self.0.saturating_add(rhs.0))
    }
}

impl Add<u64> for TransactionWeight {
    type Output = TransactionWeight;

    fn add(self, rhs: u64) -> Self::Output {
        TransactionWeight(self.0.saturating_add(rhs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_addition_and_sum_saturate_on_overflow() {
        let max = TransactionWeight::new(u64::MAX);
        assert_eq!((max + TransactionWeight::new(1)).as_u64(), u64::MAX);
        assert_eq!((max + 10u64).as_u64(), u64::MAX);

        let summed: TransactionWeight = [u64::MAX, 1u64, 100u64].into_iter().sum();
        assert_eq!(summed.as_u64(), u64::MAX);
    }
}
