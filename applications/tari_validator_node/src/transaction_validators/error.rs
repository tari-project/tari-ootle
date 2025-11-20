//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_networking::NetworkingError;
use tari_ootle_common_types::{Epoch, Network};
use tari_ootle_storage::{consensus_models::TransactionPoolError, StorageError};
use tari_template_lib::types::TemplateAddress;
use tari_transaction::TransactionId;

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
    #[error("Dry run transactions are not allowed")]
    DryRunNotAllowed,
}
