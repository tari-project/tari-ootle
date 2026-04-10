//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::{Transaction, UnsealedTransaction, UnsignedTransaction};

use crate::{
    transaction::TransactionSealSigner,
    wallet::{TransactionAuthorization, WalletResult},
};

#[derive(Clone, Debug, Default)]
pub struct Initial;
pub struct WithTx(UnsignedTransaction);

/// A builder for constructing signed transactions ready for submission.
///
/// Follows a typestate pattern: start with [`TransactionRequest::new()`], attach an
/// unsigned transaction with [`with_transaction()`](TransactionRequest::with_transaction),
/// optionally add extra authorizations, then call [`build()`](TransactionRequest::build)
/// to seal and sign.
///
/// ```rust,ignore
/// let tx = TransactionRequest::default()
///     .with_transaction(unsigned_tx)
///     .build(provider.wallet())
///     .await?;
/// ```
#[derive(Clone, Debug, Default)]
pub struct TransactionRequest<State = Initial> {
    state: State,
    authorizations: Vec<TransactionAuthorization>,
}

impl TransactionRequest<Initial> {
    pub fn new() -> Self {
        Self {
            state: Initial,
            authorizations: Vec::new(),
        }
    }

    pub fn with_transaction(self, builder: UnsignedTransaction) -> TransactionRequest<WithTx> {
        TransactionRequest {
            state: WithTx(builder),
            authorizations: self.authorizations,
        }
    }
}

impl<State> TransactionRequest<State> {
    pub fn add_authorization(mut self, auth: TransactionAuthorization) -> Self {
        self.authorizations.push(auth);
        self
    }

    pub fn with_authorizations<I>(mut self, auths: I) -> Self
    where I: IntoIterator<Item = TransactionAuthorization> {
        self.authorizations.extend(auths);
        self
    }
}

impl TransactionRequest<WithTx> {
    pub fn build_unsealed(self) -> UnsealedTransaction {
        self.state.0.finish()
    }

    pub async fn build(self, seal_signer: &dyn TransactionSealSigner) -> WalletResult<Transaction> {
        let builder = self.state.0;
        let unsealed = builder.with_signatures(self.authorizations.into_iter().map(|a| a.into_signature()).collect());
        let final_tx = seal_signer.seal_transaction(unsealed).await?;
        Ok(final_tx)
    }
}
