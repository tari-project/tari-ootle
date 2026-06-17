//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::{Network, Transaction};

use crate::{
    BasicValidations,
    TransactionNetworkValidator,
    TransactionSignatureValidator,
    TransactionValidationError,
    TransactionWeightValidator,
    Validator,
};

/// Builds the structural (context-free) mempool validations suitable for any transaction entry
/// point: network match, basic well-formedness, the per-transaction weight cap, and signature
/// verification.
///
/// These never depend on lagging runtime state (epoch, template existence), so they cannot
/// false-reject and are safe to run at the indexer before forwarding to validator committees. The
/// validator node composes the same validators plus the context-dependent ones (dry-run rejection,
/// template existence, epoch range).
pub fn create_structural_transaction_validator(
    network: Network,
    max_transaction_weight: u64,
) -> impl Validator<Transaction, Context = (), Error = TransactionValidationError> {
    TransactionNetworkValidator::new(network)
        .and_then(BasicValidations::new())
        .and_then(TransactionWeightValidator::new(max_transaction_weight))
        .and_then(TransactionSignatureValidator)
}
