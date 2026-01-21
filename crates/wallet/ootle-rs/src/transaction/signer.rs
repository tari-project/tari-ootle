//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::{TransactionSealSignature, UnsealedTransactionV1, UnsignedTransaction};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{signer, types::Address, wallet::TransactionAuthorization};

// NOTE: async_trait is required because returning impl Future is not currently dyn compatible
#[async_trait::async_trait]
pub trait TransactionSigner {
    /// Get the public key bytes of the signer.
    fn address(&self) -> &Address;

    /// Asynchronously sign a transaction message.
    async fn sign_transaction(&self, message: &UnsealedTransactionV1) -> signer::Result<TransactionSealSignature>;

    async fn sign_authorization(
        &self,
        seal_signer: &RistrettoPublicKeyBytes,
        tx: &UnsignedTransaction,
    ) -> signer::Result<TransactionAuthorization>;
}
