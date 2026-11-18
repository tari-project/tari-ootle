//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::borrow::Cow;

use tari_template_lib_types::{
    AccessRule,
    stealth::{SpendCondition, TemplateFunction},
};

/// How a created stealth output is gated for spending (TIP-0006). `StealthPublicKey` produces a key-path output (a
/// `spend_key`); the other variants produce a condition tree (a `condition_root`). `AccessRule`/`TemplateFunction` are
/// single-leaf conveniences; `Conditions` commits a full multi-leaf tree (a MAST of alternative spend paths).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub enum PayTo {
    #[default]
    StealthPublicKey,
    AccessRule(AccessRule),
    /// Gate the output's spend on a stateless WASM predicate. The output value is still encrypted to the destination
    /// (so the recipient can discover and decrypt it), but spending it requires satisfying the predicate rather than
    /// proving ownership of a key.
    TemplateFunction(TemplateFunction),
    /// Gate the output's spend on a condition tree (MAST) of alternative spend paths. The output commits the Merkle
    /// root over these leaves; a spender later reveals exactly one leaf plus an inclusion proof.
    Conditions(Vec<SpendCondition>),
}

impl PayTo {
    /// The condition-tree leaves this intent commits, or `None` for a key-path output.
    pub fn conditions(&self) -> Option<Cow<'_, [SpendCondition]>> {
        match self {
            Self::StealthPublicKey => None,
            Self::AccessRule(rule) => Some(Cow::Owned(vec![SpendCondition::access_rule(rule.clone())])),
            Self::TemplateFunction(func) => Some(Cow::Owned(vec![SpendCondition::template_function(func.clone())])),
            Self::Conditions(conditions) => Some(Cow::Borrowed(conditions)),
        }
    }
}
