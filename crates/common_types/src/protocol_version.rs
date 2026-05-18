// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};

use crate::Epoch;

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SchemaVersion {
    V0 = 0,
}

impl SchemaVersion {
    pub const fn as_u32(self) -> u32 {
        self as u32
    }
}

impl Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "V{}", self.as_u32())
    }
}

pub struct ProtocolVersion;

impl ProtocolVersion {
    // Ordered by activation epoch ascending. Entry at index 0 is the genesis schema.
    // NB: entries here are CONSENSUS-BOUND via hash_substate. Never reorder or mutate an entry
    // after it has activated on a live network — doing so changes every hash derived under it.
    const ACTIVATIONS: &'static [(Epoch, SchemaVersion)] = &[(Epoch(0), SchemaVersion::V0)];
    pub const MAX_SUPPORTED: SchemaVersion = SchemaVersion::V0;

    pub fn at(epoch: Epoch) -> SchemaVersion {
        Self::ACTIVATIONS
            .iter()
            .rev()
            .find(|(at, _)| *at <= epoch)
            .map(|(_, v)| *v)
            .unwrap_or(SchemaVersion::V0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_is_v0() {
        assert_eq!(ProtocolVersion::at(Epoch(0)), SchemaVersion::V0);
    }

    #[test]
    fn far_future_is_max_supported() {
        assert_eq!(ProtocolVersion::at(Epoch(u64::MAX)), ProtocolVersion::MAX_SUPPORTED);
    }

    #[test]
    fn monotonic_across_activations() {
        let mut prev: Option<Epoch> = None;
        for (at, _) in ProtocolVersion::ACTIVATIONS {
            if let Some(p) = prev {
                assert!(*at >= p, "activations table must be sorted ascending by epoch");
            }
            prev = Some(*at);
        }
    }
}
