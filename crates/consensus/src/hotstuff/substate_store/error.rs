//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::{VersionedSubstateId, optional::IsNotFoundError};
use tari_ootle_storage::{StorageError, consensus_models::LockConflict};

#[derive(Debug, thiserror::Error)]
pub enum SubstateStoreError {
    #[error("Lock failure: {0}")]
    LockFailed(#[from] LockFailedError),
    /// The substate has no live version at the requested version - it was destroyed, or was never
    /// created. Which of the two cannot be distinguished without retaining every version ever created,
    /// so the two must never be reported apart: the reason string reaches consensus through
    /// `RejectReason`, and a node that has pruned its history would otherwise abort with a different
    /// reason than one that has not.
    #[error("Substate {id} is not found or DOWN")]
    SubstateNotFound { id: VersionedSubstateId },
    #[error("Expected substate {id} to be DOWN but it was UP")]
    ExpectedSubstateDown { id: VersionedSubstateId },

    #[error(transparent)]
    StoreError(#[from] StorageError),
    #[error(transparent)]
    StateTreeError(#[from] tari_state_tree::StateTreeError),
    #[error("Invariant error: {details}")]
    InvariantError { details: String },
}

impl IsNotFoundError for SubstateStoreError {
    fn is_not_found_error(&self) -> bool {
        match self {
            SubstateStoreError::LockFailed(LockFailedError::SubstateNotFound { .. }) => true,
            SubstateStoreError::SubstateNotFound { .. } => true,
            SubstateStoreError::StoreError(err) => err.is_not_found_error(),
            _ => false,
        }
    }
}

impl SubstateStoreError {
    pub fn ok_lock_failed(self) -> Result<LockFailedError, Self> {
        match self {
            SubstateStoreError::LockFailed(err) => Ok(err),
            // A substate with no live version is an expected conflict (its version was already spent by an
            // earlier transaction, or it never existed), not a fatal store error. Treat it as a lock failure
            // so callers can skip or abort the transaction rather than aborting consensus.
            SubstateStoreError::SubstateNotFound { id } => Ok(LockFailedError::SubstateNotFound { id }),
            other => Err(other),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LockFailedError {
    #[error("Substate {id} is not found or DOWN")]
    SubstateNotFound { id: VersionedSubstateId },
    #[error(
        "Failed to {} lock substate {substate_id} due to conflict with existing {} lock in transaction {}", conflict.requested_lock, conflict.existing_lock, conflict.transaction_id
    )]
    LockConflict {
        substate_id: VersionedSubstateId,
        conflict: LockConflict,
    },
    /// The exact version an OUTPUT lock claims has already existed. A version that was created and later
    /// destroyed is as unusable as a live one - the version is part of the substate's identity, so recreating
    /// it would resurrect a spent substate.
    #[error("Substate {id} already exists and cannot be created as an output")]
    SubstateExists { id: VersionedSubstateId },
}

impl LockFailedError {
    pub fn lock_conflict(&self) -> Option<&LockConflict> {
        match self {
            LockFailedError::LockConflict { conflict, .. } => Some(conflict),
            _ => None,
        }
    }
}
