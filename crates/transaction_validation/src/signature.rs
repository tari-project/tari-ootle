//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use log::warn;
use tari_ootle_transaction::Transaction;

use crate::{TransactionValidationError, Validator};

const LOG_TARGET: &str = "tari::ootle::mempool::validators::signature";

#[derive(Debug)]
pub struct TransactionSignatureValidator;

impl Validator<Transaction> for TransactionSignatureValidator {
    type Context = ();
    type Error = TransactionValidationError;

    fn validate(&self, _context: &(), transaction: &Transaction) -> Result<(), TransactionValidationError> {
        if transaction.main_signer().is_none() {
            warn!(target: LOG_TARGET, "TransactionSignatureValidator - FAIL: No main signer");
            return Err(TransactionValidationError::NoMainSigner {
                transaction_id: transaction.calculate_id(),
            });
        }

        if !transaction.verify_all_signatures() {
            warn!(target: LOG_TARGET, "TransactionSignatureValidator - FAIL: Invalid signature");
            return Err(TransactionValidationError::InvalidSignature);
        }

        Ok(())
    }
}
