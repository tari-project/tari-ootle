//    Copyright 2026 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};

use super::SpendAuthorization;
use crate::{
    Hash32,
    crypto::{PedersenCommitmentBytes, UtxoTag},
};

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
/// Confidential values remain hidden — only the commitment, `minimum_value_promise`, the output's
/// [`SpendAuthorization`] and tag are visible. This is exactly what enables covenants: a predicate can assert
/// properties of the outputs it produces (e.g. that they preserve its own `condition_root`).
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StealthOutputView {
    #[n(0)]
    pub commitment: PedersenCommitmentBytes,
    #[n(1)]
    pub minimum_value_promise: u64,
    #[n(2)]
    pub auth: SpendAuthorization,
    #[n(3)]
    pub tag: UtxoTag,
}

/// Identifies the input whose spend condition is currently executing, including the `condition_root` committed by the
/// UTXO being spent (so a covenant predicate can require outputs to preserve it).
#[derive(Debug, Clone, Encode, Decode, CborLen, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CurrentInputView {
    #[n(0)]
    pub index: u32,
    #[n(1)]
    pub commitment: PedersenCommitmentBytes,
    /// The committed condition-tree root of the UTXO being spent. Always `Some` while a script-path predicate runs.
    #[n(2)]
    pub condition_root: Option<Hash32>,
}
