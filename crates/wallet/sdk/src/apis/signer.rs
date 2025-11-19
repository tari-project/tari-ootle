//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use tari_ootle_common_types::{IntoSigned, Signable};

use crate::{
    apis::key_manager::{KeyManagerApi, KeyManagerApiError},
    key_managers::{SignatureOutput, WalletKeyStore},
    models::KeyId,
    spec::KeyStoreError,
    storage::WalletStorageError,
    WalletSdkSpec,
};

pub struct SignerApi<'a, TSpec: WalletSdkSpec> {
    key_manager: KeyManagerApi<'a, TSpec>,
}

impl<'a, TSpec: WalletSdkSpec> SignerApi<'a, TSpec> {
    pub fn new(key_manager: KeyManagerApi<'a, TSpec>) -> Self {
        Self { key_manager }
    }

    pub fn get_signature<T, CTX>(
        &mut self,
        key_id: KeyId,
        context: CTX,
        item: &T,
    ) -> Result<SignatureOutput, SignerApiError<TSpec>>
    where
        T: Signable<CTX>,
    {
        match key_id {
            KeyId::Derived { key_branch, index } => {
                let output = self
                    .key_manager
                    .key_store()
                    .sign(key_branch.as_str(), index, context, item)
                    // NOTE: Cannot implement From due to rust bug/limitation
                    .map_err(SignerApiError::KeyStoreError)?;
                Ok(output)
            },
            KeyId::Imported { local_key_id } => {
                // TODO: do we actually need to support signing from an imported key? Typically these are view-only
                // keys. If we removed support for this, we'd just need the key store as a dependency of the signing api
                // instead of the key manager api.
                let key = self.key_manager.get_key(KeyId::imported(local_key_id))?;
                let sig = key.sign(context, item);
                Ok(SignatureOutput {
                    public_key: key.to_public_key(),
                    signature: sig,
                })
            },
        }
    }

    pub fn sign_with_context<T, Ctx>(
        &mut self,
        key_id: KeyId,
        context: Ctx,
        item: T,
    ) -> Result<T::SignedOutput, SignerApiError<TSpec>>
    where
        T: IntoSigned<Ctx>,
    {
        let output = self.get_signature(key_id, context, &item)?;
        let output = item.into_signed(output.public_key, output.signature);
        Ok(output)
    }

    pub fn sign<T>(&mut self, key_id: KeyId, item: T) -> Result<T::SignedOutput, SignerApiError<TSpec>>
    where T: IntoSigned<()> {
        let output = self.get_signature(key_id, (), &item)?;
        let output = item.into_signed(output.public_key, output.signature);
        Ok(output)
    }
}

impl<TSpec> Clone for SignerApi<'_, TSpec>
where
    TSpec: WalletSdkSpec,
    TSpec::KeyStore: Clone,
{
    fn clone(&self) -> Self {
        Self {
            key_manager: self.key_manager.clone(),
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
