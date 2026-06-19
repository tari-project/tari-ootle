//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib_types::{AccessRule, stealth::TemplateFunction};

/// How a created stealth output is gated for spending (TIP-0006). `StealthPublicKey` produces a key-path output (a
/// `spend_key`); the other variants produce a single-leaf condition tree (a `condition_root`).
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
}
