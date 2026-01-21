//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_ootle_common_types::signature::SignatureOutput;
use tari_ootle_transaction::{IntoSigned, Signable};

use crate::{
    apis::key_manager::{KeyManagerApi, KeyManagerApiError},
    models::{KeyId, StealthUtxoSpendKeyId},
    spec::KeyStoreError,
    storage::WalletStorageError,
    WalletSdkSpec,
};

pub struct SignerApi<'a, TSpec: WalletSdkSpec, Ctx = ()> {
    key_manager: KeyManagerApi<'a, TSpec>,
    context: Ctx,
}

impl<'a, TSpec: WalletSdkSpec> SignerApi<'a, TSpec, ()> {
    pub fn new(key_manager: KeyManagerApi<'a, TSpec>) -> Self {
        Self {
            key_manager,
            context: (),
        }
    }

    pub fn with_context<Ctx>(self, context: Ctx) -> SignerApi<'a, TSpec, Ctx> {
        SignerApi {
            key_manager: self.key_manager,
            context,
        }
    }
}

impl<'a, TSpec: WalletSdkSpec, Ctx: Copy> SignerApi<'a, TSpec, Ctx> {
    pub fn generate_signature<T>(&self, key_id: KeyId, item: &T) -> Result<T::Signature, SignerApiError<TSpec>>
    where
        T: Signable<Ctx>,
        T::Signature: From<SignatureOutput>,
    {
        let output = self.key_manager.sign_with_context(key_id, self.context, item)?;
        Ok(output)
    }

    pub fn generate_stealth_key_signature<T>(
        &self,
        key_id: &StealthUtxoSpendKeyId,
        item: &T,
    ) -> Result<T::Signature, SignerApiError<TSpec>>
    where
        T: Signable<Ctx>,
        T::Signature: From<SignatureOutput>,
    {
        let output = self.key_manager.sign_with_stealth_key(key_id, self.context, item)?;
        Ok(output)
    }

    pub fn sign<T>(&self, key_id: KeyId, item: T) -> Result<T::SignedOutput, SignerApiError<TSpec>>
    where
        T: IntoSigned<Ctx>,
        <T as Signable<Ctx>>::Signature: From<SignatureOutput>,
    {
        let sig = self.generate_signature(key_id, &item)?;
        let output = item.into_signed(sig);
        Ok(output)
    }

    pub fn sign_with_stealth_key<T>(
        &self,
        key_id: &StealthUtxoSpendKeyId,
        item: T,
    ) -> Result<T::SignedOutput, SignerApiError<TSpec>>
    where
        T: IntoSigned<Ctx>,
        <T as Signable<Ctx>>::Signature: From<SignatureOutput>,
    {
        let sig = self.generate_stealth_key_signature(key_id, &item)?;
        let output = item.into_signed(sig);
        Ok(output)
    }

    pub fn sign_with_explicit_key<T>(
        &self,
        secret_key: &RistrettoSecretKey,
        item: T,
    ) -> Result<T::SignedOutput, SignerApiError<TSpec>>
    where
        T: IntoSigned<Ctx>,
        <T as Signable<Ctx>>::Signature: From<SignatureOutput>,
    {
        let sig = self
            .key_manager
            .sign_with_explicit_key(secret_key, self.context, &item)?;
        let output = item.into_signed(sig);
        Ok(output)
    }
}

impl<TSpec, Ctx> Clone for SignerApi<'_, TSpec, Ctx>
where
    TSpec: WalletSdkSpec,
    TSpec::KeyStore: Clone,
    Ctx: Clone,
{
    fn clone(&self) -> Self {
        Self {
            key_manager: self.key_manager.clone(),
            context: self.context.clone(),
        }
    }
}

#[derive(thiserror::Error)]
pub enum SignerApiError<TSpec: WalletSdkSpec> {
    #[error("Key store error: {0}")]
    KeyStoreError(KeyStoreError<TSpec>),
    #[error("Wallet storage error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Key manager API error: {0}")]
    KeyManagerError(#[from] KeyManagerApiError),
}

impl<TSpec: WalletSdkSpec> fmt::Debug for SignerApiError<TSpec> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyStoreError(e) => write!(f, "KeyStoreError: {:?}", e),
            Self::StoreError(e) => write!(f, "StoreError: {:?}", e),
            Self::KeyManagerError(e) => write!(f, "KeyManagerError: {:?}", e),
        }
    }
}
