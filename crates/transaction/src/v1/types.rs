//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

/// Possible allocatable address types
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, borsh::BorshSerialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum AllocatableAddressType {
    #[n(0)]
    Component,
    #[n(1)]
    Resource,
}
