// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::warn;
use tari_ootle_transaction::{MAX_TRANSACTION_INSTRUCTIONS, Transaction};

use crate::{TransactionValidationError, Validator};

const LOG_TARGET: &str = "tari::ootle::mempool::validators::instruction_limits";

/// Rejects transactions carrying more than [`MAX_TRANSACTION_INSTRUCTIONS`] instructions, fee instructions
/// included.
///
/// A transaction off the wire cannot exceed the bound in either list, since decoding refuses it. This
/// bounds the two together and covers a transaction built locally or deserialized from JSON, which no peer
/// could decode if it were relayed.
#[derive(Debug, Clone, Default)]
pub struct InstructionLimitValidator;

impl InstructionLimitValidator {
    pub fn new() -> Self {
        Self
    }
}

impl Validator<Transaction> for InstructionLimitValidator {
    type Context = ();
    type Error = TransactionValidationError;

    fn validate(&self, _context: &(), transaction: &Transaction) -> Result<(), Self::Error> {
        let count = transaction.fee_instructions().len() + transaction.instructions().len();
        if count > MAX_TRANSACTION_INSTRUCTIONS {
            let transaction_id = transaction.calculate_id();
            warn!(
                target: LOG_TARGET,
                "InstructionLimitValidator - FAIL: {transaction_id} carries {count} instructions, maximum is {MAX_TRANSACTION_INSTRUCTIONS}"
            );
            return Err(TransactionValidationError::TooManyInstructions {
                transaction_id,
                max: MAX_TRANSACTION_INSTRUCTIONS,
                actual: count,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::PrivateKey;
    use tari_ootle_common_types::Epoch;
    use tari_ootle_transaction::Instruction;

    use super::*;

    fn tx_with_instructions(fee_instructions: usize, instructions: usize) -> Transaction {
        Transaction::builder_localnet(Epoch(10))
            .with_fee_instructions((0..fee_instructions).map(|_| Instruction::DropAllProofsInWorkspace))
            .with_instructions((0..instructions).map(|_| Instruction::DropAllProofsInWorkspace))
            .build_and_seal(&PrivateKey::from(1u64))
    }

    #[test]
    fn accepts_instructions_at_the_limit() {
        InstructionLimitValidator::new()
            .validate(&(), &tx_with_instructions(1, MAX_TRANSACTION_INSTRUCTIONS - 1))
            .unwrap();
    }

    #[test]
    fn rejects_instructions_over_the_limit_across_both_lists() {
        let err = InstructionLimitValidator::new()
            .validate(&(), &tx_with_instructions(1, MAX_TRANSACTION_INSTRUCTIONS))
            .unwrap_err();
        assert!(matches!(
            err,
            TransactionValidationError::TooManyInstructions { max, actual, .. }
            if max == MAX_TRANSACTION_INSTRUCTIONS && actual == MAX_TRANSACTION_INSTRUCTIONS + 1
        ));
    }
}
