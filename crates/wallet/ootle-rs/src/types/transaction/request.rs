//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::{Transaction, UnsealedTransactionV1, UnsignedTransaction};

use crate::wallet::{OotleWallet, TransactionAuthorization, WalletResult};

#[derive(Clone, Debug, Default)]
pub struct Initial;
pub struct WithTx(UnsignedTransaction);

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

impl TransactionRequest<WithTx> {
    pub fn add_authorization(mut self, auth: TransactionAuthorization) -> Self {
        self.authorizations.push(auth);
        self
    }

    pub fn build_unsealed(self) -> UnsealedTransactionV1 {
        self.state.0.finish()
    }

    pub async fn build(self, wallet: &OotleWallet) -> WalletResult<Transaction> {
        let builder = self.state.0;
        let unsealed = builder.with_signatures(self.authorizations.into_iter().map(|a| a.into_signature()).collect());
        let final_tx = wallet.sign_transaction(unsealed).await?;
        Ok(final_tx)
    }
}
