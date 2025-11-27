//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_transaction::Transaction;

use crate::{transaction_validators::TransactionValidationError, validator::Validator};

const LOG_TARGET: &str = "tari::ootle::mempool::validators::is_shard_applicable";

/// Basic validations for a transaction:
/// - Has at least one fee instruction
#[derive(Debug, Clone, Default)]
pub struct BasicValidations;

impl BasicValidations {
    pub fn new() -> Self {
        Self
    }
}

impl Validator<Transaction> for BasicValidations {
    type Context = ();
    type Error = TransactionValidationError;

    fn validate(&self, _context: &(), transaction: &Transaction) -> Result<(), Self::Error> {
        if transaction.fee_instructions().is_empty() {
            warn!(target: LOG_TARGET, "BasicValidations - FAIL: No fee instructions");
            return Err(TransactionValidationError::NoFeeInstructions {
                transaction_id: transaction.calculate_id(),
            });
        }

        if transaction.instructions().iter().any(|instr| instr.is_pay_fee()) {
            warn!(target: LOG_TARGET, "BasicValidations - FAIL: Transaction contains pay fee instruction");
            return Err(TransactionValidationError::ContainsPayFeeInstruction {
                transaction_id: transaction.calculate_id(),
            });
        }

        // TODO: additional checks?

        debug!(target: LOG_TARGET, "BasicValidations - OK");
        Ok(())
    }
}
