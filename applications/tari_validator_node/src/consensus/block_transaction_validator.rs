//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use tari_consensus::traits::BlockTransactionValidator;
use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::Transaction;
use tari_ootle_transaction_validation::{BoxedValidator, TransactionValidationError, Validator};

/// Validators whose checks depend only on the transaction itself. These are immutable over a transaction's
/// lifetime (signature, network, weight, template existence, ...) and need only be run once.
pub type StructuralTransactionValidator = BoxedValidator<(), Transaction, TransactionValidationError>;

/// Validators whose checks depend on the current epoch, which advances while a transaction waits to be
/// sequenced, so they must be re-run at sequencing time.
pub type EpochTransactionValidator = BoxedValidator<Epoch, Transaction, TransactionValidationError>;

/// Splits transaction validation into its structural (content-only) and epoch-dependent parts so that consensus
/// can re-run only what is necessary: peer-requested transactions are validated in full, while mempool-originated
/// transactions — already structurally validated on ingress — are only re-checked against the current epoch.
#[derive(Clone)]
pub struct TariBlockTransactionValidator {
    structural: Arc<StructuralTransactionValidator>,
    epoch: Arc<EpochTransactionValidator>,
}

impl TariBlockTransactionValidator {
    pub fn new(structural: StructuralTransactionValidator, epoch: EpochTransactionValidator) -> Self {
        Self {
            structural: Arc::new(structural),
            epoch: Arc::new(epoch),
        }
    }
}

impl BlockTransactionValidator for TariBlockTransactionValidator {
    type Error = TransactionValidationError;

    fn validate_full(&self, current_epoch: Epoch, transaction: &Transaction) -> Result<(), Self::Error> {
        self.structural.validate(&(), transaction)?;
        self.epoch.validate(&current_epoch, transaction)?;
        Ok(())
    }

    fn validate_epoch(&self, current_epoch: Epoch, transaction: &Transaction) -> Result<(), Self::Error> {
        self.epoch.validate(&current_epoch, transaction)
    }
}
