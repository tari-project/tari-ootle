//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use log::warn;
use tari_ootle_common_types::services::template_provider::TemplateProvider;
use tari_ootle_transaction::Transaction;

use crate::{TransactionValidationError, Validator};

const LOG_TARGET: &str = "tari::ootle::mempool::validators::template_exists";

#[derive(Debug)]
pub struct TemplateExistsValidator<TProvider> {
    provider: TProvider,
}

impl<TProvider> TemplateExistsValidator<TProvider> {
    pub fn new(provider: TProvider) -> Self {
        Self { provider }
    }
}

impl<TProvider: TemplateProvider> Validator<Transaction> for TemplateExistsValidator<TProvider> {
    type Context = ();
    type Error = TransactionValidationError;

    fn validate(&self, _context: &(), transaction: &Transaction) -> Result<(), TransactionValidationError> {
        let templates = transaction.referenced_templates_iter();
        for address in templates {
            if !self.provider.has_template(address).map_err(|e| {
                warn!(target: LOG_TARGET, "TemplateExistsValidator - FAIL: Template lookup error: {}", e);
                TransactionValidationError::TemplateLookupError { source: e.into() }
            })? {
                warn!(target: LOG_TARGET, "TemplateExistsValidator - FAIL: Template not found");
                return Err(TransactionValidationError::TemplateNotFound { address: *address });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, convert::Infallible};

    use indexmap::IndexSet;
    use tari_ootle_transaction::{
        ComponentReference,
        Instruction,
        Network,
        Sr25519SignatureBytes,
        TransactionSealSignature,
        UnsealedTransactionV1,
        UnsignedTransactionV1,
    };
    use tari_template_lib::types::{ComponentAddress, ObjectKey, TemplateAddress, crypto::RistrettoPublicKeyBytes};

    use super::*;

    #[derive(Clone)]
    struct KnownTemplates(HashSet<TemplateAddress>);

    impl TemplateProvider for KnownTemplates {
        type Error = Infallible;
        type Template = ();

        fn get_template(&self, address: &TemplateAddress) -> Result<Option<()>, Infallible> {
            Ok(self.0.contains(address).then_some(()))
        }
    }

    fn update_component_template(new_template: TemplateAddress) -> Transaction {
        let instruction = Instruction::UpdateComponentTemplate {
            component: ComponentReference::Address(ComponentAddress::new(ObjectKey::from_array([1; 32]))),
            migrate: None,
            new_template,
        };
        Transaction::new(
            UnsealedTransactionV1::new(
                UnsignedTransactionV1::new(
                    Network::LocalNet.as_byte(),
                    vec![],
                    vec![instruction],
                    IndexSet::new(),
                    None,
                    tari_ootle_common_types::Epoch(10),
                    false,
                ),
                vec![],
            )
            .into(),
            TransactionSealSignature::new(RistrettoPublicKeyBytes::zero(), Sr25519SignatureBytes::zero()),
        )
    }

    #[test]
    fn a_component_template_update_to_an_unknown_template_is_rejected() {
        let known = TemplateAddress::from_array([1; 32]);
        let unknown = TemplateAddress::from_array([2; 32]);
        let validator = TemplateExistsValidator::new(KnownTemplates(HashSet::from([known])));

        validator.validate(&(), &update_component_template(known)).unwrap();
        let err = validator
            .validate(&(), &update_component_template(unknown))
            .unwrap_err();
        assert!(
            matches!(err, TransactionValidationError::TemplateNotFound { address } if address == unknown),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a_stealth_script_path_spend_referencing_an_unknown_template_is_rejected() {
        use tari_ootle_transaction::ResourceAddressRef;
        use tari_template_lib::types::{
            Amount,
            constants::TARI_TOKEN,
            crypto::PedersenCommitmentBytes,
            stealth::{
                MerkleProof,
                SpendCondition,
                SpendWitness,
                StealthInput,
                StealthInputsStatement,
                StealthOutputsStatement,
                StealthTransferStatement,
                TemplateFunction,
            },
        };

        let known = TemplateAddress::from_array([1; 32]);
        let unknown = TemplateAddress::from_array([2; 32]);
        let validator = TemplateExistsValidator::new(KnownTemplates(HashSet::from([known])));

        let stealth_tx = |template: TemplateAddress| {
            let func = TemplateFunction::new(template, "check".try_into().unwrap(), vec![]);
            let leaf = SpendCondition::template_function(func);
            let witness = SpendWitness::script_path(leaf, MerkleProof::empty());
            let input = StealthInput::with_witness(PedersenCommitmentBytes::zero(), witness);
            let outputs_statement =
                StealthOutputsStatement::new_revealed_only(Amount::from(10u64), RistrettoPublicKeyBytes::zero());
            let statement = StealthTransferStatement {
                inputs_statement: StealthInputsStatement::new(vec![input], Amount::ZERO),
                outputs_statement,
                balance_proof: None,
                covenant_claims: vec![],
            };
            let instruction = Instruction::StealthTransfer {
                resource_address_ref: ResourceAddressRef::Address(TARI_TOKEN),
                statement,
                revealed_input_bucket: None,
            };
            Transaction::new(
                UnsealedTransactionV1::new(
                    UnsignedTransactionV1::new(
                        Network::LocalNet.as_byte(),
                        vec![],
                        vec![instruction],
                        IndexSet::new(),
                        None,
                        tari_ootle_common_types::Epoch(10),
                        false,
                    ),
                    vec![],
                )
                .into(),
                TransactionSealSignature::new(RistrettoPublicKeyBytes::zero(), Sr25519SignatureBytes::zero()),
            )
        };

        validator.validate(&(), &stealth_tx(known)).unwrap();
        let err = validator.validate(&(), &stealth_tx(unknown)).unwrap_err();
        assert!(
            matches!(err, TransactionValidationError::TemplateNotFound { address } if address == unknown),
            "unexpected error: {err}"
        );
    }
}
