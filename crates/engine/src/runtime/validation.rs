//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::limits::StealthLimits;
use tari_template_lib::types::stealth::{StealthOutputsStatement, StealthTransferStatement};

use crate::runtime::error::ArgumentValidationError;

pub(crate) fn check_stealth_transfer_limits(
    limits: &StealthLimits,
    statement: &StealthTransferStatement,
) -> Result<(), ArgumentValidationError> {
    if statement.inputs_statement.inputs.len() > limits.max_inputs {
        return Err(ArgumentValidationError::MaxStealthInputsExceeded {
            max_inputs: limits.max_inputs,
            actual_inputs: statement.inputs_statement.inputs.len(),
        });
    }
    check_stealth_outputs_limits(limits, &statement.outputs_statement)?;
    Ok(())
}

pub(crate) fn check_stealth_outputs_limits(
    limits: &StealthLimits,
    statement: &StealthOutputsStatement,
) -> Result<(), ArgumentValidationError> {
    if statement.outputs.len() > limits.max_outputs {
        return Err(ArgumentValidationError::MaxStealthOutputsExceeded {
            max_outputs: limits.max_outputs,
            actual_outputs: statement.outputs.len(),
        });
    }
    Ok(())
}

/// Running per-transaction tally of stealth-transfer work. Bounds the aggregate native verification cost a single
/// transaction can incur across all its `StealthTransfer` instructions, so one transaction cannot stall the proposing
/// leader. See [`StealthLimits`] and [`tari_engine_types::limits::STEALTH_LIMITS`].
#[derive(Debug, Clone, Default)]
pub(crate) struct StealthTransactionTotals {
    transfers: usize,
    inputs: usize,
    outputs: usize,
}

impl StealthTransactionTotals {
    /// Accounts one more stealth transfer against the per-transaction caps. Returns an error — which aborts the
    /// transaction before the transfer's (expensive) crypto runs — if any cap would be exceeded. Totals only advance
    /// when the transfer is admitted, so the work actually performed never exceeds the caps.
    pub fn account_transfer(
        &mut self,
        limits: &StealthLimits,
        statement: &StealthTransferStatement,
    ) -> Result<(), ArgumentValidationError> {
        let transfers = self.transfers + 1;
        if transfers > limits.max_transfers_per_transaction {
            return Err(ArgumentValidationError::MaxStealthTransfersPerTransactionExceeded {
                max_transfers: limits.max_transfers_per_transaction,
            });
        }
        let inputs = self.inputs + statement.inputs_statement.inputs.len();
        if inputs > limits.max_total_inputs_per_transaction {
            return Err(ArgumentValidationError::MaxStealthInputsPerTransactionExceeded {
                max_inputs: limits.max_total_inputs_per_transaction,
                actual_inputs: inputs,
            });
        }
        let outputs = self.outputs + statement.outputs_statement.outputs.len();
        if outputs > limits.max_total_outputs_per_transaction {
            return Err(ArgumentValidationError::MaxStealthOutputsPerTransactionExceeded {
                max_outputs: limits.max_total_outputs_per_transaction,
                actual_outputs: outputs,
            });
        }
        self.transfers = transfers;
        self.inputs = inputs;
        self.outputs = outputs;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tari_template_lib::types::{
        Amount,
        EncryptedData,
        crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, UtxoTag},
        stealth::{SpendCondition, StealthInput, StealthUnspentOutput, UnspentOutput},
    };

    use super::*;

    const LIMITS: StealthLimits = StealthLimits {
        max_inputs: 1000,
        max_outputs: 8,
        max_transfers_per_transaction: 2,
        max_total_inputs_per_transaction: 3,
        max_total_outputs_per_transaction: 2,
    };

    fn dummy_commitment() -> PedersenCommitmentBytes {
        PedersenCommitmentBytes::from_bytes(&[0u8; 32]).unwrap()
    }

    fn dummy_output() -> StealthUnspentOutput {
        StealthUnspentOutput {
            output: UnspentOutput {
                commitment: dummy_commitment(),
                sender_public_nonce: RistrettoPublicKeyBytes::zero(),
                encrypted_data: EncryptedData::try_from(vec![0u8; EncryptedData::min_size()]).unwrap(),
                minimum_value_promise: 0,
                viewable_balance_proof: None,
            },
            spend_condition: SpendCondition::Signed(RistrettoPublicKeyBytes::zero()),
            tag: UtxoTag::new(0),
        }
    }

    /// A stealth transfer statement with the given input/output counts; only the counts matter to the accumulator.
    fn statement(n_inputs: usize, n_outputs: usize) -> StealthTransferStatement {
        let mut stmt = StealthTransferStatement::revealed_only(Amount::new(1), Amount::new(1));
        stmt.inputs_statement.inputs = (0..n_inputs).map(|_| StealthInput::new(dummy_commitment())).collect();
        stmt.outputs_statement.outputs = (0..n_outputs).map(|_| dummy_output()).collect();
        stmt
    }

    #[test]
    fn admits_a_transfer_sitting_on_every_cap() {
        let mut totals = StealthTransactionTotals::default();
        totals.account_transfer(&LIMITS, &statement(3, 2)).unwrap();
    }

    #[test]
    fn rejects_the_transfer_that_exceeds_the_transfer_cap() {
        let mut totals = StealthTransactionTotals::default();
        totals.account_transfer(&LIMITS, &statement(0, 0)).unwrap();
        totals.account_transfer(&LIMITS, &statement(0, 0)).unwrap();
        let err = totals.account_transfer(&LIMITS, &statement(0, 0)).unwrap_err();
        assert!(matches!(
            err,
            ArgumentValidationError::MaxStealthTransfersPerTransactionExceeded { max_transfers: 2 }
        ));
    }

    #[test]
    fn sums_inputs_and_outputs_across_transfers() {
        let mut totals = StealthTransactionTotals::default();
        totals.account_transfer(&LIMITS, &statement(2, 1)).unwrap();
        let err = totals.account_transfer(&LIMITS, &statement(2, 0)).unwrap_err();
        assert!(matches!(
            err,
            ArgumentValidationError::MaxStealthInputsPerTransactionExceeded { max_inputs: 3, .. }
        ));
    }

    #[test]
    fn a_rejected_transfer_does_not_consume_budget() {
        let mut totals = StealthTransactionTotals::default();
        totals.account_transfer(&LIMITS, &statement(2, 0)).unwrap();
        // Rejected: 2 + 2 = 4 > 3 inputs. Must not advance the running totals.
        totals.account_transfer(&LIMITS, &statement(2, 0)).unwrap_err();
        // A 1-input transfer still fits (2 + 1 = 3).
        totals.account_transfer(&LIMITS, &statement(1, 0)).unwrap();
    }
}
