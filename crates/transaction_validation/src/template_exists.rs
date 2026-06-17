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
