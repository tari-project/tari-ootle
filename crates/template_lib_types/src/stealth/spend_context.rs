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

impl StealthOutputView {
    /// Whether this output is authorised by exactly `Script(condition_root)` — a pure condition-tree lock with no key
    /// path. A `KeyAndScript` output committing the same root is key-spendable next block, so it does not keep value
    /// under the covenant and is not considered locked under the root.
    pub fn is_locked_under(&self, condition_root: &Hash32) -> bool {
        match &self.auth {
            SpendAuthorization::Script(root) => root == condition_root,
            SpendAuthorization::Key(_) | SpendAuthorization::KeyAndScript { .. } => false,
        }
    }
}

/// "Stay in the vault" covenant predicate: there is at least one stealth output and every one is re-locked under
/// exactly `Script(condition_root)` (no key-path escape). Backs both the native `OutputPreservesCondition` builtin and
/// the `SpendContext::require_output_preserves_condition` helper, so the host and guest evaluate it identically.
pub fn outputs_preserve_condition(outputs: &[StealthOutputView], condition_root: &Hash32) -> bool {
    !outputs.is_empty() && outputs.iter().all(|o| o.is_locked_under(condition_root))
}

/// Value-flow covenant predicate: at least one stealth output is locked under exactly `Script(condition_root)` (no
/// key-path escape) and promises at least `min_value`. Backs both the native `OutputTo` builtin and the
/// `SpendContext::require_output_to` helper.
pub fn has_output_to(outputs: &[StealthOutputView], condition_root: &Hash32, min_value: u64) -> bool {
    outputs
        .iter()
        .any(|o| o.is_locked_under(condition_root) && o.minimum_value_promise >= min_value)
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
