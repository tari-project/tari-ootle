//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_networking::NetworkingError;
use tari_ootle_common_types::Epoch;
use tari_ootle_storage::{StorageError, consensus_models::TransactionPoolError};
use tari_ootle_transaction::{BlobValidationError, Network, TransactionId};
use tari_template_lib::types::TemplateAddress;

#[derive(thiserror::Error, Debug)]
pub enum TransactionValidationError {
    #[error("Storage Error: {0}")]
    StorageError(#[from] StorageError),
    #[error("Transaction pool error: {0}")]
    TransactionPoolError(#[from] TransactionPoolError),
    #[error("Template lookup error: {source}")]
    TemplateLookupError { source: anyhow::Error },

    // TODO: move these to MempoolValidationError type
    #[error("Template not found: {address}")]
    TemplateNotFound { address: TemplateAddress },
    #[error("{transaction_id} has no fee instructions")]
    NoFeeInstructions { transaction_id: TransactionId },
    #[error("Output substate exists in transaction {transaction_id}")]
    OutputSubstateExists { transaction_id: TransactionId },
    #[error("Validator fee claim instruction in transaction {transaction_id} contained invalid epoch {given_epoch}")]
    ValidatorFeeClaimEpochInvalid {
        transaction_id: TransactionId,
        given_epoch: Epoch,
    },
    #[error("Current epoch ({current_epoch}) is less than minimum epoch ({min_epoch}) required for transaction")]
    CurrentEpochLessThanMinimum { current_epoch: Epoch, min_epoch: Epoch },
    #[error("Current epoch ({current_epoch}) is greater than maximum epoch ({max_epoch}) required for transaction")]
    CurrentEpochGreaterThanMaximum { current_epoch: Epoch, max_epoch: Epoch },
    #[error("Invalid transaction signature")]
    InvalidSignature,
    #[error("Transaction {transaction_id} has no main signer")]
    NoMainSigner { transaction_id: TransactionId },
    #[error("Transaction {transaction_id} is not signed")]
    TransactionNotSigned { transaction_id: TransactionId },
    #[error("Network error: {0}")]
    NetworkingError(#[from] NetworkingError),
    #[error("Unknown network byte \"{byte}\": {details}")]
    UnknownNetwork { byte: u8, details: String },
    #[error("Network mismatch! Current network: {actual}, Transaction network: {expected}")]
    NetworkMismatch { actual: Network, expected: Network },
    #[error("Transaction {transaction_id} contains a pay fee instruction, which is not allowed")]
    ContainsPayFeeInstruction { transaction_id: TransactionId },
    #[error("Dry run transactions are not allowed")]
    DryRunNotAllowed,
    #[error("Transaction {transaction_id} weight {weight} exceeds the maximum allowed weight {max_weight}")]
    TransactionExceedsMaxWeight {
        transaction_id: TransactionId,
        weight: u64,
        max_weight: u64,
    },
    #[error("Transaction {transaction_id} exceeds the per-transaction stealth {limit} cap: max {max}, got {actual}")]
    ExceedsStealthTransactionLimit {
        transaction_id: TransactionId,
        limit: &'static str,
        max: usize,
        actual: usize,
    },
    #[error(
        "Transaction {transaction_id} contains {actual} publish-template instructions, but the maximum allowed is \
         {max}"
    )]
    TooManyPublishTemplateInstructions {
        transaction_id: TransactionId,
        max: usize,
        actual: usize,
    },
    #[error("Transaction {transaction_id} contains {actual} signatures, but the maximum allowed is {max}")]
    TooManySignatures {
        transaction_id: TransactionId,
        max: usize,
        actual: usize,
    },
    #[error("Transaction {transaction_id} has invalid blob references: {source}")]
    InvalidBlobReferences {
        transaction_id: TransactionId,
        source: BlobValidationError,
    },
}

impl TransactionValidationError {
    /// Whether this failure is attributable to whoever sent us the transaction.
    ///
    /// A transaction arriving over gossip is rejected back to the mesh when it is, and merely
    /// ignored when it is not: rejection counts against the sending peer's score and can graylist
    /// it, so it must be reserved for failures that peer is responsible for.
    ///
    /// The distinction is whether the verdict could differ between two honest nodes. Structural
    /// failures — malformed, wrong network, over a cap, bad signature — are properties of the
    /// transaction itself and every node agrees on them. Failures that depend on this node's view
    /// of runtime state, or on this node's own health, do not: a transaction referencing a template
    /// we have not synced yet is valid to a peer that has, and a database error is our problem
    /// entirely. Penalising a peer for either would let lagging or unhealthy nodes graylist honest
    /// ones, and a node with a failing store would graylist everything it talks to.
    ///
    /// Matched exhaustively so that a new variant has to make this choice deliberately.
    pub fn is_sender_fault(&self) -> bool {
        match self {
            // Ours, not theirs: local storage, pool and networking failures say nothing about the
            // transaction.
            Self::StorageError(_) |
            Self::TransactionPoolError(_) |
            Self::TemplateLookupError { .. } |
            Self::NetworkingError(_) => false,

            // Depends on this node's view of runtime state, which legitimately lags behind a peer's.
            Self::TemplateNotFound { .. } |
            Self::OutputSubstateExists { .. } |
            Self::ValidatorFeeClaimEpochInvalid { .. } |
            Self::CurrentEpochLessThanMinimum { .. } |
            Self::CurrentEpochGreaterThanMaximum { .. } => false,

            // Properties of the transaction itself, on which every node agrees.
            Self::NoFeeInstructions { .. } |
            Self::InvalidSignature |
            Self::NoMainSigner { .. } |
            Self::TransactionNotSigned { .. } |
            Self::UnknownNetwork { .. } |
            Self::NetworkMismatch { .. } |
            Self::ContainsPayFeeInstruction { .. } |
            Self::DryRunNotAllowed |
            Self::TransactionExceedsMaxWeight { .. } |
            Self::ExceedsStealthTransactionLimit { .. } |
            Self::TooManyPublishTemplateInstructions { .. } |
            Self::TooManySignatures { .. } |
            Self::InvalidBlobReferences { .. } => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tx_id() -> TransactionId {
        TransactionId::new([1; 32])
    }

    /// A peer that is ahead of us, or our own unhealthy node, must not be able to graylist honest
    /// peers. These are the failures where our verdict can differ from another honest node's.
    #[test]
    fn node_local_failures_are_not_blamed_on_the_sender() {
        let node_local = [
            TransactionValidationError::TemplateNotFound {
                address: TemplateAddress::from_array([1; 32]),
            },
            TransactionValidationError::TemplateLookupError {
                source: anyhow::anyhow!("db unavailable"),
            },
            TransactionValidationError::OutputSubstateExists {
                transaction_id: tx_id(),
            },
            TransactionValidationError::CurrentEpochLessThanMinimum {
                current_epoch: Epoch(1),
                min_epoch: Epoch(2),
            },
            TransactionValidationError::CurrentEpochGreaterThanMaximum {
                current_epoch: Epoch(3),
                max_epoch: Epoch(2),
            },
        ];
        for err in node_local {
            assert!(!err.is_sender_fault(), "must not penalise the sender for: {err}");
        }
    }

    /// Properties of the transaction itself: every honest node reaches the same verdict, so the
    /// peer that sent it is answerable for it.
    #[test]
    fn structural_failures_are_blamed_on_the_sender() {
        let structural = [
            TransactionValidationError::InvalidSignature,
            TransactionValidationError::NoMainSigner {
                transaction_id: tx_id(),
            },
            TransactionValidationError::NoFeeInstructions {
                transaction_id: tx_id(),
            },
            TransactionValidationError::ContainsPayFeeInstruction {
                transaction_id: tx_id(),
            },
            TransactionValidationError::DryRunNotAllowed,
            TransactionValidationError::TransactionExceedsMaxWeight {
                transaction_id: tx_id(),
                weight: 2,
                max_weight: 1,
            },
            TransactionValidationError::TooManySignatures {
                transaction_id: tx_id(),
                max: 1,
                actual: 2,
            },
            TransactionValidationError::InvalidBlobReferences {
                transaction_id: tx_id(),
                source: BlobValidationError::UnreferencedBlob { index: 0 },
            },
        ];
        for err in structural {
            assert!(err.is_sender_fault(), "must penalise the sender for: {err}");
        }
    }
}
