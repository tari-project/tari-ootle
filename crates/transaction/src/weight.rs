//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{iter::Sum, ops::Add};

#[derive(Debug, Clone, Copy, Default)]
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
        Self(iter.sum())
    }
}

impl Add for TransactionWeight {
    type Output = TransactionWeight;

    fn add(self, rhs: Self) -> Self::Output {
        TransactionWeight(self.0 + rhs.0)
    }
}

impl Add<u64> for TransactionWeight {
    type Output = TransactionWeight;

    fn add(self, rhs: u64) -> Self::Output {
        TransactionWeight(self.0 + rhs)
    }
}
