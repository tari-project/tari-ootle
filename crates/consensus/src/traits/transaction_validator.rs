//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::Epoch;
use tari_ootle_transaction::Transaction;

/// Validates transactions before they are admitted to the transaction pool.
///
/// Validation is intentionally separate from [`super::BlockTransactionExecutor`]: it is a pure function of the
/// transaction and the current epoch, with no dependency on execution or substate state. The two entry points
/// reflect how much validation a transaction still requires:
///
/// * [`validate_full`](BlockTransactionValidator::validate_full) — the transaction comes from an untrusted source (e.g.
///   requested from a peer) and has not yet been validated locally, so it must be validated in full.
/// * [`validate_epoch`](BlockTransactionValidator::validate_epoch) — the transaction has already passed full validation
///   locally (e.g. via the mempool). Its structural properties are immutable, so only the epoch/time-dependent rules —
///   which can change while the transaction waits to be sequenced — are re-checked.
pub trait BlockTransactionValidator {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Fully validate a transaction from an untrusted source.
    fn validate_full(&self, current_epoch: Epoch, transaction: &Transaction) -> Result<(), Self::Error>;

    /// Re-validate only the epoch/time-dependent rules of an already fully-validated transaction.
    fn validate_epoch(&self, current_epoch: Epoch, transaction: &Transaction) -> Result<(), Self::Error>;
}
