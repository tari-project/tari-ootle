//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use log::warn;
use tari_dan_common_types::NodeAddressable;
use tari_dan_storage::global::TemplateStatus;
use tari_template_manager::{implementation::TemplateManager, interface::TemplateManagerError};
use tari_transaction::Transaction;

use crate::{transaction_validators::TransactionValidationError, validator::Validator};

const LOG_TARGET: &str = "tari::dan::mempool::validators::template_exists";

#[derive(Debug)]
pub struct TemplateExistsValidator<TAddr> {
    template_manager: TemplateManager<TAddr>,
}

impl<TAddr> TemplateExistsValidator<TAddr> {
    pub(crate) fn new(template_manager: TemplateManager<TAddr>) -> Self {
        Self { template_manager }
    }
}

impl<TAddr: NodeAddressable> Validator<Transaction> for TemplateExistsValidator<TAddr> {
    type Context = ();
    type Error = TransactionValidationError;

    fn validate(&self, _context: &(), transaction: &Transaction) -> Result<(), TransactionValidationError> {
        let instructions = transaction.instructions().iter().chain(transaction.fee_instructions());
        for instruction in instructions {
            if let Some(template_address) = instruction.referenced_template() {
                // Template may be Pending sync. In this case, we can process the transaction at the consensus
                // level, but not execute.
                let template_exists = self.template_manager.template_exists(template_address, None)?;
                if !template_exists {
                    warn!(target: LOG_TARGET, "TemplateExistsValidator - FAIL: Template not found");
                    return Err(TransactionValidationError::InvalidTemplateAddress(
                        TemplateManagerError::TemplateNotFound {
                            address: *template_address,
                        },
                    ));
                }

                let template_is_invalid = self
                    .template_manager
                    .template_exists(template_address, Some(TemplateStatus::Invalid))?;
                if template_is_invalid {
                    // This should only be possible when templates come from L1
                    warn!(target: LOG_TARGET, "TemplateExistsValidator - FAIL: Template {template_address} is invalid");
                    return Err(TransactionValidationError::InvalidTemplateAddress(
                        TemplateManagerError::InvalidBaseLayerTemplate,
                    ));
                }
            }
        }

        Ok(())
    }
}
