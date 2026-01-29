//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::Not;

use async_trait::async_trait;
use tari_ootle_transaction::{Transaction, UnsealedTransaction, UnsignedTransaction};

use crate::{
    signer,
    stealth::SignatureRequirements,
    transaction::{ephemeral_signer::EphemeralKeySigner, TransactionSealSigner},
    wallet::{NetworkWallet, OotleWallet, TransactionAuthorization, WalletResult},
    Address,
};

pub struct WalletStealthAuthorizer<'a, W: ?Sized> {
    wallet: &'a W,
    required_signatures: SignatureRequirements,
}

impl<'a, W: ?Sized> WalletStealthAuthorizer<'a, W> {
    pub fn new(wallet: &'a W, required_signatures: SignatureRequirements) -> Self {
        Self {
            wallet,
            required_signatures,
        }
    }
}

impl WalletStealthAuthorizer<'_, OotleWallet> {
    pub async fn create_authorizations(
        &self,
        unsigned: &UnsignedTransaction,
    ) -> signer::Result<Vec<TransactionAuthorization>> {
        let seal_signer = if self.required_signatures.must_sign_with_account_key() {
            // We'll seal with the wallet's default key (seal_signer() is None)
            Some(self.wallet.default_address().account_public_key())
        } else {
            // We'll seal with the seal signer
            self.required_signatures
                .seal_signer()
                .map(|s| s.signer().account_public_key())
        };

        let Some(seal_signer) = seal_signer else {
            // There are no inputs to sign for
            return Ok(vec![]);
        };

        let mut authorizations = Vec::with_capacity(self.required_signatures.len().saturating_sub(1));
        for req in self.required_signatures.other_signers() {
            let auth = self
                .wallet
                .authorize_transaction_with_stealth_key(req.signer(), req.public_nonce(), seal_signer, unsigned)
                .await?;
            authorizations.push(auth);
        }
        Ok(authorizations)
    }
}

#[async_trait]
impl TransactionSealSigner for WalletStealthAuthorizer<'_, OotleWallet> {
    async fn seal_transaction(&self, transaction: UnsealedTransaction) -> signer::Result<Transaction> {
        let stealth_seal_signer = self
            .required_signatures
            .must_sign_with_account_key()
            .not()
            .then(|| self.required_signatures.seal_signer())
            .map(|s| s.or_else(|| self.required_signatures.other_signers().next()));

        match stealth_seal_signer {
            Some(Some(seal_signer)) => {
                // Seal with a stealth key. This signature is required to spend an input.
                self.wallet
                    .seal_transaction_with_stealth_key(seal_signer.signer(), seal_signer.public_nonce(), &transaction)
                    .await
            },
            Some(None) => {
                // CASE: must_sign_with_account_key is false, but we have no inputs to spend (therefore no required
                // signers) we can sign with "any" key (the transaction needs a seal signer). For privacy, we generate
                // an ephemeral key rather than, say, using the wallet key.
                Ok(EphemeralKeySigner::random().seal_transaction(transaction))
            },
            None => {
                // Seal with the wallet's default key
                self.wallet.seal_transaction(transaction).await
            },
        }
    }
}

impl NetworkWallet for WalletStealthAuthorizer<'_, OotleWallet> {
    fn default_address(&self) -> &Address {
        self.wallet.default_address()
    }

    async fn sign_transaction(&self, unsigned: UnsignedTransaction) -> WalletResult<Transaction> {
        let authorizations = self.create_authorizations(&unsigned).await?;
        let unsealed = unsigned.with_signatures(authorizations.into_iter().map(|a| a.into_signature()).collect());

        let stealth_seal_signer = self
            .required_signatures
            .must_sign_with_account_key()
            .not()
            .then(|| self.required_signatures.seal_signer())
            .map(|s| {
                // We should either have must_sign_with_account_key = true or at least one signer
                s.ok_or_else(|| {
                    signer::SignerError::other("No signers found in required_signatures for stealth sealing")
                })
            })
            .transpose()?;

        let tx = if let Some(seal_signer) = stealth_seal_signer {
            // Seal with a stealth key
            self.wallet
                .seal_transaction_with_stealth_key(seal_signer.signer(), seal_signer.public_nonce(), &unsealed)
                .await?
        } else {
            // Seal with the wallet's default key
            self.wallet.seal_transaction(unsealed).await?
        };
        Ok(tx)
    }
}
