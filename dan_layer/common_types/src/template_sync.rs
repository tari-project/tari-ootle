// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::substate::SubstateId;

use crate::VersionedSubstateId;

/// A request for a template to be downloaded from another shard group that owns it.
#[derive(Debug, Clone)]
pub struct TemplateSyncRequest {
    /// Versioned substate ID parsed from a transaction input.
    substate_id: VersionedSubstateId,
}

impl TemplateSyncRequest {
    pub fn new(substate_id: SubstateId, version: u32) -> Self {
        Self {
            substate_id: VersionedSubstateId::new(substate_id, version),
        }
    }

    pub fn substate_id(&self) -> &VersionedSubstateId {
        &self.substate_id
    }
}
