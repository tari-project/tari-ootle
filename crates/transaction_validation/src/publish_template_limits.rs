// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::warn;
use tari_engine_types::limits::MAX_PUBLISH_TEMPLATES_PER_TRANSACTION;
use tari_ootle_transaction::{Instruction, Transaction};

use crate::{TransactionValidationError, Validator};

const LOG_TARGET: &str = "tari::ootle::mempool::validators::publish_template_limits";

/// Rejects transactions carrying more than [`MAX_PUBLISH_TEMPLATES_PER_TRANSACTION`] `PublishTemplate` instructions.
///
/// This mirrors the engine's execution-time cap at ingress, rejecting such transactions before they are gossiped,
/// stored and executed. The engine remains the consensus authority; see
/// [`tari_engine_types::limits::MAX_PUBLISH_TEMPLATES_PER_TRANSACTION`].
#[derive(Debug, Clone, Default)]
pub struct PublishTemplateLimitValidator;

impl PublishTemplateLimitValidator {
    pub fn new() -> Self {
        Self
    }
}

impl Validator<Transaction> for PublishTemplateLimitValidator {
    type Context = ();
    type Error = TransactionValidationError;

    fn validate(&self, _context: &(), transaction: &Transaction) -> Result<(), Self::Error> {
        // Count across both instruction lists, matching `Transaction::has_publish_template`.
        let count = transaction
            .instructions()
            .iter()
            .chain(transaction.fee_instructions())
            .filter(|instruction| matches!(instruction, Instruction::PublishTemplate { .. }))
            .count();

        if count > MAX_PUBLISH_TEMPLATES_PER_TRANSACTION {
            let transaction_id = transaction.calculate_id();
            warn!(
                target: LOG_TARGET,
                "PublishTemplateLimitValidator - FAIL: {transaction_id} has {count} publish-template instructions, \
                 maximum is {MAX_PUBLISH_TEMPLATES_PER_TRANSACTION}"
            );
            return Err(TransactionValidationError::TooManyPublishTemplateInstructions {
                transaction_id,
                max: MAX_PUBLISH_TEMPLATES_PER_TRANSACTION,
                actual: count,
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
        TransactionSealSignature,
        TransactionSignature,
        UnsealedTransactionV1,
        UnsignedTransactionV1,
    };
    use tari_template_lib::types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

    use super::*;

    fn publish_template() -> Instruction {
        Instruction::PublishTemplate {
            binary: 0,
            metadata_hash: None,
        }
    }

    fn tx_with_instructions(instructions: Vec<Instruction>) -> Transaction {
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
    fn accepts_no_publish_template() {
        let tx = tx_with_instructions(vec![]);
        PublishTemplateLimitValidator::new().validate(&(), &tx).unwrap();
    }

    #[test]
    fn accepts_single_publish_template() {
        let tx = tx_with_instructions(vec![publish_template()]);
        PublishTemplateLimitValidator::new().validate(&(), &tx).unwrap();
    }

    #[test]
    fn rejects_multiple_publish_templates() {
        let tx = tx_with_instructions(vec![publish_template(), publish_template()]);
        let err = PublishTemplateLimitValidator::new().validate(&(), &tx).unwrap_err();
        assert!(matches!(
            err,
            TransactionValidationError::TooManyPublishTemplateInstructions { max: 1, actual: 2, .. }
        ));
    }
}
