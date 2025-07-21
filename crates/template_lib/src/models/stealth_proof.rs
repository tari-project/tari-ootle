//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_template_lib_types::crypto::RangeProofBytes;

use crate::models::ConfidentialStatement;

/// A statement for stealth outputs. A statement must contain confidential outputs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct StealthOutputStatement {
    /// Proof of the confidential resources that are going to be transferred to the receiver
    pub outputs: Vec<ConfidentialStatement>,
    /// Bulletproof range proof for the output commitments proving that values are in the range
    /// [minimum_value_promise, 2^64)
    pub range_proof: RangeProofBytes,
}
