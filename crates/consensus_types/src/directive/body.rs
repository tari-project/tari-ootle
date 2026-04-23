//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use tari_ootle_common_types::Epoch;

/// The authenticated payload of a [`crate::ConsensusDirective`].
///
/// The body is canonically serialised with borsh when computing the [`crate::DirectiveId`]
/// and when signing. `nonce` and `issued_at_unix_secs` together make each emission unique;
/// validators persist applied directive IDs so replay of an already-applied directive is
/// detected and short-circuited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DirectiveBody {
    pub kind: DirectiveKind,
    pub nonce: u64,
    pub issued_at_unix_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum DirectiveKind {
    /// Roll consensus back to the epoch checkpoint for `target_epoch`.
    ///
    /// The receiving validator must already hold a valid, QC-verified `EpochCheckpoint`
    /// for `target_epoch`; directive delivery does not supply or override the checkpoint.
    ///
    /// `target_epoch` is stored as a `u64` rather than [`Epoch`] so the body can be
    /// canonicalised with borsh (which [`Epoch`] does not currently implement). Use
    /// [`DirectiveKind::rollback_to_epoch`] / [`DirectiveKind::target_epoch`] to interop.
    RollbackToEpochCheckpoint { target_epoch: u64 },
}

impl DirectiveKind {
    pub fn rollback_to_epoch(epoch: Epoch) -> Self {
        Self::RollbackToEpochCheckpoint { target_epoch: epoch.0 }
    }

    pub fn target_epoch(&self) -> Option<Epoch> {
        match self {
            Self::RollbackToEpochCheckpoint { target_epoch } => Some(Epoch(*target_epoch)),
        }
    }
}
