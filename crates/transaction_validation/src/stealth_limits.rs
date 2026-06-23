// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::warn;
use tari_engine_types::limits::STEALTH_LIMITS;
use tari_ootle_transaction::{Instruction, Transaction};

use crate::{TransactionValidationError, Validator};

const LOG_TARGET: &str = "tari::ootle::mempool::validators::stealth_limits";

/// Rejects transactions whose aggregate stealth-transfer work exceeds the per-transaction caps in `STEALTH_LIMITS`.
///
/// Verifying a stealth transfer is native, unmetered crypto (a bulletproof range proof plus an ElGamal proof per
/// output), so without a per-transaction cap a single transaction could stack enough verification to push the
/// proposing leader past the block time. The engine enforces the same caps during execution; this rejects such
/// transactions at ingress, before they are gossiped, stored and executed.
#[derive(Debug, Clone, Default)]
pub struct StealthTransactionLimitsValidator;

impl StealthTransactionLimitsValidator {
    pub fn new() -> Self {
        Self
    }
}

impl Validator<Transaction> for StealthTransactionLimitsValidator {
    type Context = ();
    type Error = TransactionValidationError;

    fn validate(&self, _context: &(), transaction: &Transaction) -> Result<(), Self::Error> {
        let mut transfers = 0usize;
        let mut inputs = 0usize;
        let mut outputs = 0usize;

        for instruction in transaction.instructions().iter().chain(transaction.fee_instructions()) {
            if let Instruction::StealthTransfer { statement, .. } = instruction {
                transfers += 1;
                inputs += statement.inputs_statement.inputs.len();
                outputs += statement.outputs_statement.outputs.len();
            }
        }

        self.check(
            "transfers",
            transfers,
            STEALTH_LIMITS.max_transfers_per_transaction,
            transaction,
        )?;
        self.check(
            "inputs",
            inputs,
            STEALTH_LIMITS.max_total_inputs_per_transaction,
            transaction,
        )?;
        self.check(
            "outputs",
            outputs,
            STEALTH_LIMITS.max_total_outputs_per_transaction,
            transaction,
        )?;
        Ok(())
    }
}

impl StealthTransactionLimitsValidator {
    fn check(
        &self,
        limit: &'static str,
        actual: usize,
        max: usize,
        transaction: &Transaction,
    ) -> Result<(), TransactionValidationError> {
        if actual > max {
            let transaction_id = transaction.calculate_id();
            warn!(
                target: LOG_TARGET,
                "StealthTransactionLimitsValidator - FAIL: {transaction_id} stealth {limit} {actual} exceeds maximum {max}"
            );
            return Err(TransactionValidationError::ExceedsStealthTransactionLimit {
                transaction_id,
                limit,
                max,
                actual,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexSet;
    use tari_ootle_transaction::{
        Network,
        ResourceAddressRef,
        TransactionSealSignature,
        TransactionSignature,
        UnsealedTransactionV1,
        UnsignedTransactionV1,
    };
    use tari_template_lib::types::{
        Amount,
        EncryptedData,
        crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, SchnorrSignatureBytes, UtxoTag},
        stealth::{SpendCondition, StealthInput, StealthTransferStatement, StealthUnspentOutput, UnspentOutput},
    };

    use super::*;

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

    /// A stealth transfer statement with the given input/output counts. Only the counts matter to this validator, so
    /// the commitments and proofs are dummies.
    fn statement(n_inputs: usize, n_outputs: usize) -> StealthTransferStatement {
        // A non-zero revealed amount keeps the statement constructor happy; only the input/output counts matter here.
        let mut stmt = StealthTransferStatement::revealed_only(Amount::new(1), Amount::new(1));
        stmt.inputs_statement.inputs = (0..n_inputs).map(|_| StealthInput::new(dummy_commitment())).collect();
        stmt.outputs_statement.outputs = (0..n_outputs).map(|_| dummy_output()).collect();
        stmt
    }

    fn tx_with_stealth_transfers(statements: Vec<StealthTransferStatement>) -> Transaction {
        let instructions = statements
            .into_iter()
            .map(|statement| Instruction::StealthTransfer {
                resource_address_ref: ResourceAddressRef::from(0u16),
                statement,
                revealed_input_bucket: None,
            })
            .collect();
        Transaction::new(
            UnsealedTransactionV1::new(
                UnsignedTransactionV1::new(
                    Network::LocalNet.as_byte(),
                    vec![],
                    instructions,
                    IndexSet::new(),
                    None,
                    None,
                    false,
                ),
                vec![TransactionSignature::new(
                    RistrettoPublicKeyBytes::zero(),
                    SchnorrSignatureBytes::zero(),
                )],
            )
            .into(),
            TransactionSealSignature::new(RistrettoPublicKeyBytes::zero(), SchnorrSignatureBytes::zero()),
        )
    }

    #[test]
    fn accepts_transaction_at_the_caps() {
        let limits = STEALTH_LIMITS;
        // One transfer that sits exactly on the input and output caps, plus filler transfers up to the transfer cap.
        let mut statements = vec![statement(
            limits.max_total_inputs_per_transaction,
            limits.max_total_outputs_per_transaction,
        )];
        statements.extend((1..limits.max_transfers_per_transaction).map(|_| statement(0, 0)));
        let tx = tx_with_stealth_transfers(statements);
        StealthTransactionLimitsValidator::new().validate(&(), &tx).unwrap();
    }

    #[test]
    fn rejects_too_many_transfers() {
        let n = STEALTH_LIMITS.max_transfers_per_transaction + 1;
        let tx = tx_with_stealth_transfers((0..n).map(|_| statement(0, 0)).collect());
        let err = StealthTransactionLimitsValidator::new().validate(&(), &tx).unwrap_err();
        assert!(matches!(
            err,
            TransactionValidationError::ExceedsStealthTransactionLimit { limit: "transfers", .. }
        ));
    }

    #[test]
    fn rejects_too_many_total_inputs() {
        let tx = tx_with_stealth_transfers(vec![statement(STEALTH_LIMITS.max_total_inputs_per_transaction + 1, 0)]);
        let err = StealthTransactionLimitsValidator::new().validate(&(), &tx).unwrap_err();
        assert!(matches!(
            err,
            TransactionValidationError::ExceedsStealthTransactionLimit { limit: "inputs", .. }
        ));
    }

    #[test]
    fn rejects_too_many_total_outputs_summed_across_transfers() {
        // Split the over-cap output total across two transfers to prove the validator sums across the transaction.
        let per = STEALTH_LIMITS.max_total_outputs_per_transaction / 2 + 1;
        let tx = tx_with_stealth_transfers(vec![statement(0, per), statement(0, per)]);
        let err = StealthTransactionLimitsValidator::new().validate(&(), &tx).unwrap_err();
        assert!(matches!(
            err,
            TransactionValidationError::ExceedsStealthTransactionLimit { limit: "outputs", .. }
        ));
    }
}
