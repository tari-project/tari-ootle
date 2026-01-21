//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, sync::Arc};

use tari_ootle_transaction::{
    IntoSigned,
    Transaction,
    TransactionSignature,
    UnsealedTransactionV1,
    UnsignedTransaction,
};

use crate::{transaction::TransactionSigner, types::Address, wallet::error::WalletError};

type AddressHashMap<T> = HashMap<Address, T>;
pub type WalletResult<T> = Result<T, WalletError>;

#[derive(Clone)]
pub struct OotleWallet {
    default: Address,
    signers: AddressHashMap<Arc<dyn TransactionSigner + Send + Sync>>,
}

impl std::fmt::Debug for OotleWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OotleWallet")
            .field("default", &self.default)
            .field("num_credentials", &self.signers.len())
            .finish()
    }
}

impl<S> From<S> for OotleWallet
where S: TransactionSigner + Send + Sync + 'static
{
    fn from(signer: S) -> Self {
        Self::new(signer)
    }
}

impl OotleWallet {
    /// Create a new wallet with the given signer as the default signer.
    pub fn new<S>(signer: S) -> Self
    where S: TransactionSigner + Send + Sync + 'static {
        let mut this = Self {
            default: signer.address().clone(),
            signers: Default::default(),
        };
        this.register_signer(signer);
        this
    }

    /// Register a new signer on this object, and set it as the default signer.
    /// This signer will be used to sign [`TransactionRequest`] that does not
    /// specify a signer address in the `from` field.
    /// [`TransactionRequest`]: crate::api_types::TransactionRequest
    pub fn register_signer<S>(&mut self, signer: S)
    where S: TransactionSigner + Send + Sync + 'static {
        self.signers.insert(signer.address().clone(), Arc::new(signer));
    }

    pub async fn authorize_transaction(
        &self,
        address: &Address,
        unsigned: &UnsignedTransaction,
    ) -> WalletResult<TransactionAuthorization> {
        let default_address = self.default_signer_address();
        let signer = self.signers.get(address).ok_or_else(|| WalletError::SignerNotFound {
            address: address.clone(),
        })?;
        let signature = signer
            .sign_authorization(default_address.account_public_key(), unsigned)
            .await?;
        Ok(signature)
    }

    pub async fn sign_transaction(&self, unsealed: UnsealedTransactionV1) -> WalletResult<Transaction> {
        let signer = self
            .signers
            .get(self.default_signer_address())
            .ok_or_else(|| WalletError::SignerNotFound {
                address: self.default_signer_address().clone(),
            })?;
        let signature = signer.sign_transaction(&unsealed).await?;
        Ok(<UnsealedTransactionV1 as IntoSigned>::into_signed(unsealed, signature))
    }

    pub fn additional_signers(&self) -> impl Iterator<Item = &Address> {
        self.signers.keys()
    }

    pub fn default_signer_address(&self) -> &Address {
        &self.default
    }
}

#[derive(Clone, Debug)]
pub struct TransactionAuthorization {
    signature: TransactionSignature,
}

impl TransactionAuthorization {
    pub fn new(signature: TransactionSignature) -> Self {
        Self { signature }
    }

    pub fn signature(&self) -> &TransactionSignature {
        &self.signature
    }

    pub fn into_signature(self) -> TransactionSignature {
        self.signature
    }
}

impl From<TransactionSignature> for TransactionAuthorization {
    fn from(signature: TransactionSignature) -> Self {
        Self::new(signature)
    }
}
