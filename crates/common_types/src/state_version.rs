//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use tari_bor::{Deserialize, Serialize};

// TODO: use this new-type where appropriate in the codebase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Deserialize, Serialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct StateVersion(#[cfg_attr(feature = "ts", ts(type = "number | bigint"))] u64);

impl StateVersion {
    pub const fn new(version: u64) -> Self {
        Self(version)
    }

    pub const fn zero() -> Self {
        Self(0)
    }

    pub const fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Display for StateVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
