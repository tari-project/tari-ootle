//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_ootle_transaction::{Instruction, Transaction};

use crate::{TransactionValidationError, Validator};

const LOG_TARGET: &str = "tari::ootle::mempool::validators::is_shard_applicable";

/// Basic validations for a transaction:
/// - Has at least one fee instruction
/// - All fee instructions are permitted in the fee intent, and at least one can pay a fee
/// - Main instructions contain no fee payment instruction
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

        if let Some(index) = transaction
            .fee_instructions()
            .iter()
            .position(|instr| !instr.is_allowed_in_fee_intent())
        {
            warn!(target: LOG_TARGET, "BasicValidations - FAIL: Fee instruction {index} is not permitted");
            return Err(TransactionValidationError::DisallowedFeeInstruction {
                transaction_id: transaction.calculate_id(),
                index,
            });
        }

        if !transaction.fee_instructions().iter().any(Instruction::may_pay_fee) {
            warn!(target: LOG_TARGET, "BasicValidations - FAIL: Fee instructions cannot pay a fee");
            return Err(TransactionValidationError::NoFeePaymentInFeeInstructions {
                transaction_id: transaction.calculate_id(),
            });
        }

        if transaction.instructions().iter().any(|instr| instr.is_pay_fee()) {
            warn!(target: LOG_TARGET, "BasicValidations - FAIL: Transaction contains pay fee instruction");
            return Err(TransactionValidationError::ContainsPayFeeInstruction {
                transaction_id: transaction.calculate_id(),
            });
        }

        debug!(target: LOG_TARGET, "BasicValidations - OK");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tari_common_types::types::PrivateKey;
    use tari_ootle_common_types::Epoch;
    use tari_ootle_transaction::{ComponentReference, Instruction, args, args::WorkspaceOffsetId};
    use tari_template_lib::types::{Amount, ComponentAddress, TemplateAddress};

    use super::*;

    fn account() -> ComponentAddress {
        ComponentAddress::from_array([1u8; 32])
    }

    #[test]
    fn accepts_valid_fee_payment_instructions() {
        let tx = Transaction::builder_localnet(Epoch(10))
            .pay_fee_from_component(account(), Amount::new(1000))
            .build_and_seal(&PrivateKey::from(1u64));
        BasicValidations::new().validate(&(), &tx).unwrap();
    }

    #[test]
    fn rejects_fee_instructions_that_cannot_pay_a_fee() {
        let tx = Transaction::builder_localnet(Epoch(10))
            .with_fee_instructions([Instruction::DropAllProofsInWorkspace])
            .build_and_seal(&PrivateKey::from(1u64));
        let err = BasicValidations::new().validate(&(), &tx).unwrap_err();
        assert!(matches!(
            err,
            TransactionValidationError::NoFeePaymentInFeeInstructions { .. }
        ));
        assert!(err.is_sender_fault());
    }

    #[test]
    fn rejects_component_template_update_in_fee_instructions() {
        let tx = Transaction::builder_localnet(Epoch(10))
            .with_fee_instructions([
                Instruction::UpdateComponentTemplate {
                    component: ComponentReference::Address(account()),
                    migrate: None,
                    new_template: TemplateAddress::from_array([2u8; 32]),
                },
                Instruction::PayFeeFromBucket {
                    bucket: WorkspaceOffsetId::new(0),
                },
            ])
            .build_and_seal(&PrivateKey::from(1u64));
        let err = BasicValidations::new().validate(&(), &tx).unwrap_err();
        assert!(matches!(err, TransactionValidationError::DisallowedFeeInstruction {
            index: 0,
            ..
        }));
        assert!(err.is_sender_fault());
    }

    #[test]
    fn rejects_template_publish_in_fee_instructions() {
        let tx = Transaction::builder_localnet(Epoch(10))
            .with_fee_instructions_builder(|builder| {
                builder
                    .pay_fee_from_component(account(), Amount::new(1000))
                    .publish_template(vec![0u8; 8])
            })
            .build_and_seal(&PrivateKey::from(1u64));
        let err = BasicValidations::new().validate(&(), &tx).unwrap_err();
        assert!(matches!(err, TransactionValidationError::DisallowedFeeInstruction {
            index: 1,
            ..
        }));
        assert!(err.is_sender_fault());
    }

    #[test]
    fn rejects_pay_fee_from_bucket_in_main_instructions() {
        let tx = Transaction::builder_localnet(Epoch(10))
            .pay_fee_from_component(account(), Amount::new(1000))
            .add_instruction(Instruction::PayFeeFromBucket {
                bucket: WorkspaceOffsetId::new(0),
            })
            .build_and_seal(&PrivateKey::from(1u64));
        let err = BasicValidations::new().validate(&(), &tx).unwrap_err();
        assert!(matches!(
            err,
            TransactionValidationError::ContainsPayFeeInstruction { .. }
        ));
        assert!(err.is_sender_fault());
    }

    /// Method names belong to the template author, so a main-intent call to a method named `pay_fee` is admitted.
    /// The engine refuses any fee payment made after the fee checkpoint.
    #[test]
    fn accepts_pay_fee_named_method_in_main_instructions() {
        for method in ["pay_fee", "pay_fee_stealth"] {
            let tx = Transaction::builder_localnet(Epoch(10))
                .pay_fee_from_component(account(), Amount::new(1000))
                .call_method(account(), method, args![Amount::new(100)])
                .build_and_seal(&PrivateKey::from(1u64));
            BasicValidations::new().validate(&(), &tx).unwrap();
        }
    }
}
