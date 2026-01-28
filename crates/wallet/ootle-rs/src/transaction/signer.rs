//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use async_trait::async_trait;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_ootle_transaction::{Transaction, TransactionSealSignature, UnsealedTransaction, UnsignedTransaction};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{signer, types::Address, wallet::TransactionAuthorization};

// NOTE: async_trait is required because returning impl Future is not currently dyn compatible
#[async_trait::async_trait]
pub trait TransactionSigner {
    /// Get the public key bytes of the signer.
    fn address(&self) -> &Address;

    /// Asynchronously sign a transaction message.
    async fn sign_transaction(&self, message: &UnsealedTransaction) -> signer::Result<TransactionSealSignature>;

    async fn sign_authorization(
        &self,
        seal_signer: &RistrettoPublicKeyBytes,
        tx: &UnsignedTransaction,
    ) -> signer::Result<TransactionAuthorization>;
}

#[async_trait]
pub trait TransactionSealSigner {
    /// Asynchronously sign (seal) an unsealed transaction.
    async fn seal_transaction(&self, transaction: UnsealedTransaction) -> signer::Result<Transaction>;
}

#[async_trait]
pub trait TransactionStealthKeySigner {
    async fn sign_authorization_with_stealth(
        &self,
        public_nonce: &RistrettoPublicKey,
        seal_signer: &RistrettoPublicKeyBytes,
        tx: &UnsignedTransaction,
    ) -> signer::Result<TransactionAuthorization>;

    async fn seal_transaction_with_stealth(
        &self,
        public_nonce: &RistrettoPublicKey,
        message: &UnsealedTransaction,
    ) -> signer::Result<TransactionSealSignature>;
}
