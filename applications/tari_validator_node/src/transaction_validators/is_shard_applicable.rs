//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_transaction::Transaction;

use crate::{transaction_validators::TransactionValidationError, validator::Validator};

const LOG_TARGET: &str = "tari::ootle::mempool::validators::is_shard_applicable";

/// Refuse to process the transaction if it does not apply to any shard (i.e. does not have any inputs or claim burn
/// tombstones).
#[derive(Debug, Clone, Default)]
pub struct IsShardApplicable;

impl IsShardApplicable {
    pub fn new() -> Self {
        Self
    }
}

impl Validator<Transaction> for IsShardApplicable {
    type Context = ();
    type Error = TransactionValidationError;

    fn validate(&self, _context: &(), transaction: &Transaction) -> Result<(), Self::Error> {
        if !transaction.has_inputs() {
            warn!(target: LOG_TARGET, "HasInputs - FAIL: No input shards");
            return Err(TransactionValidationError::NoInputs {
                transaction_id: transaction.calculate_id(),
            });
        }

        debug!(target: LOG_TARGET, "HasInputs - OK");
        Ok(())
    }
}
