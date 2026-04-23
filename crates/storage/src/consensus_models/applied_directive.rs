//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_consensus_types::{BlockId, DirectiveBody, DirectiveId};
use tari_ootle_common_types::Epoch;

use crate::{StateStoreReadTransaction, StateStoreWriteTransaction, StorageError};

/// Record persisted after a governance-signed [`tari_consensus_types::ConsensusDirective`] is
/// successfully applied to a validator node.
///
/// Exists for two reasons:
/// 1. **Idempotency.** The directive orchestrator consults this table before applying; if the
///    ID is already present, the call is a no-op so operator retries cannot double-apply.
/// 2. **Audit.** The stored body + application context records what was done and when, even
///    after blocks/bookkeeping rows from that point have been rewritten by the rollback itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedDirective {
    pub directive_id: DirectiveId,
    pub body: DirectiveBody,
    pub applied_at_epoch: Epoch,
    pub applied_at_block_id: BlockId,
    pub applied_at_unix_secs: u64,
}

impl AppliedDirective {
    pub fn get<TTx: StateStoreReadTransaction>(
        tx: &TTx,
        id: &DirectiveId,
    ) -> Result<AppliedDirective, StorageError> {
        tx.applied_directive_get(id)
    }

    pub fn save<TTx: StateStoreWriteTransaction>(&self, tx: &mut TTx) -> Result<(), StorageError> {
        tx.applied_directive_save(self)
    }
}
