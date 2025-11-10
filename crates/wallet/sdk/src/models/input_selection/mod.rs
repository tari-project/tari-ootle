//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

pub mod branch_and_bound;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSelectionAlgorithm {
    /// Select the smallest number of inputs that cover the required amount
    SmallestFirst,
    /// Branch and bound algorithm
    BranchAndBound,
}
