//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerOneTransactionDef<T> {
    pub proof_type: LayerOnePayloadType,
    pub payload: T,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LayerOnePayloadType {
    EvictionProof,
}

impl Display for LayerOnePayloadType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayerOnePayloadType::EvictionProof => write!(f, "EvictionProof"),
        }
    }
}
