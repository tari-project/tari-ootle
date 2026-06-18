//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib_types::{AccessRule, stealth::SpendScript};

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, export_to = "wallet-types/"))]
pub enum PayTo {
    #[default]
    StealthPublicKey,
    AccessRule(AccessRule),
    /// Gate the output's spend on a stateless WASM predicate. The output value is still encrypted to the destination
    /// (so the recipient can discover and decrypt it), but spending it requires satisfying the script rather than
    /// proving ownership of a key.
    Script(SpendScript),
}
