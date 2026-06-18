//    Copyright 2026 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};

use super::SpendCondition;
use crate::crypto::{PedersenCommitmentBytes, UtxoTag};

/// A read-only view of a stealth input being spent, as exposed to a spend script via the `SpendContext` host op.
///
/// Confidential values remain hidden — only the commitment is visible, exactly as the balance proof operates.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StealthInputView {
    #[n(0)]
    pub commitment: PedersenCommitmentBytes,
}

/// A read-only view of a stealth output being created, as exposed to a spend script via the `SpendContext` host op.
///
/// Confidential values remain hidden — only the commitment, `minimum_value_promise`, spend condition and tag are
/// visible. This is exactly what enables covenants: a predicate can assert properties of the outputs it produces.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StealthOutputView {
    #[n(0)]
    pub commitment: PedersenCommitmentBytes,
    #[n(1)]
    pub minimum_value_promise: u64,
    #[n(2)]
    pub spend_condition: SpendCondition,
    #[n(3)]
    pub tag: UtxoTag,
}

/// Identifies the input whose spend condition is currently executing.
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CurrentInputView {
    #[n(0)]
    pub index: u32,
    #[n(1)]
    pub commitment: PedersenCommitmentBytes,
}
