//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::substate::SubstateId;

/// Answers whether a transaction may create a substate. The engine holds only a transaction's declared inputs, so on
/// its own it cannot tell that a substate it creates is already taken elsewhere.
pub trait CreationCheck {
    /// Whether `id` cannot be created by the transaction: some version of it exists in the state the transaction
    /// commits against, or a transaction that commits before it on the same chain creates it.
    fn is_creation_taken(&self, id: &SubstateId) -> Result<bool, CreationCheckError>;
}

/// Reports every creation as free, for executions with no chain to check against.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCreationCheck;

impl CreationCheck for NoCreationCheck {
    fn is_creation_taken(&self, _id: &SubstateId) -> Result<bool, CreationCheckError> {
        Ok(false)
    }
}

impl<F: Fn(&SubstateId) -> bool> CreationCheck for F {
    fn is_creation_taken(&self, id: &SubstateId) -> Result<bool, CreationCheckError> {
        Ok(self(id))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to look up substate {id}: {details}")]
pub struct CreationCheckError {
    pub id: SubstateId,
    pub details: String,
}
