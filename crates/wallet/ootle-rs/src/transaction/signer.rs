//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use async_trait::async_trait;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_ootle_transaction::{
    AuthorizationContext,
    AuthorizerSet,
    Transaction,
    TransactionSealSignature,
    UnsealedTransaction,
    UnsignedTransaction,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{signer, types::Address, wallet::TransactionAuthorization};

/// Trait for signing and authorizing transactions with a persistent key.
// NOTE: async_trait is required because returning impl Future is not currently dyn compatible
#[async_trait::async_trait]
pub trait TransactionSigner {
    /// Get the public key bytes of the signer.
    fn address(&self) -> &Address;

    /// Asynchronously sign a transaction message.
    async fn sign_transaction(&self, message: &UnsealedTransaction) -> signer::Result<TransactionSealSignature>;

    /// Authorizes `tx` under `context`, whose authorizer set must hold this signer's own key.
    async fn sign_authorization(
        &self,
        context: AuthorizationContext<'_>,
        tx: &UnsignedTransaction,
    ) -> signer::Result<TransactionAuthorization>;
}

/// Trait for applying the final seal signature to a transaction.
///
/// The seal signature is the last signature applied, proving that the transaction
/// originator approved the final set of instructions and authorizations.
#[async_trait]
pub trait TransactionSealSigner: Sync {
    /// The authorizations this signer contributes, produced before the transaction is sealed.
    ///
    /// Each one commits to the public key this signer will seal with, so only the seal signer can produce them —
    /// which is why they are made here rather than by the caller. Each also commits to the full authorizer set:
    /// `external_authorizers`, the keys of the authorizations the caller attaches, together with this signer's own.
    /// Defaults to none: a signer that seals with a key the caller already knows has nothing to add.
    async fn authorizations_for(
        &self,
        _unsigned: &UnsignedTransaction,
        _external_authorizers: &AuthorizerSet,
    ) -> signer::Result<Vec<TransactionAuthorization>> {
        Ok(Vec::new())
    }

    /// Asynchronously sign (seal) an unsealed transaction.
    async fn seal_transaction(&self, transaction: UnsealedTransaction) -> signer::Result<Transaction>;
}

/// Trait for signing transactions using derived stealth keys for confidential transactions.
#[async_trait]
pub trait TransactionStealthKeySigner {
    /// The one-time public key this signer signs with for `public_nonce`, derived without producing a signature.
    ///
    /// Every authorization signature commits to the seal signer's public key and to every authorizer's key, so a
    /// stealth input's one-time key must be resolvable before any authorization is made.
    async fn stealth_public_key(&self, public_nonce: &RistrettoPublicKey) -> signer::Result<RistrettoPublicKeyBytes>;

    /// Authorizes `tx` with the one-time key for `public_nonce` under `context`, whose authorizer set must hold
    /// that key (see [`Self::stealth_public_key`]).
    async fn sign_authorization_with_stealth(
        &self,
        public_nonce: &RistrettoPublicKey,
        context: AuthorizationContext<'_>,
        tx: &UnsignedTransaction,
    ) -> signer::Result<TransactionAuthorization>;

    async fn seal_transaction_with_stealth(
        &self,
        public_nonce: &RistrettoPublicKey,
        message: &UnsealedTransaction,
    ) -> signer::Result<TransactionSealSignature>;
}
