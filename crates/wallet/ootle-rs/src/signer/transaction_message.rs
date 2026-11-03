//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::TransactionMessage;

use crate::signer;

/// Signs transaction messages with an account key.
pub trait TransactionMessageSigner<S> {
    fn sign_transaction_message(&self, message: &TransactionMessage) -> signer::Result<S>;
}
