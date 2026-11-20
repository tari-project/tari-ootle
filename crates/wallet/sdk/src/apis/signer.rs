//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt;

use ootle_byte_type::ToByteType;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_ootle_common_types::signature::SignatureOutput;
use tari_ootle_transaction::{
    AuthorizationContext,
    AuthorizerSet,
    IntoSigned,
    NotCurrentSchema,
    SchemaVersion,
    Signable,
    UnsealedTransaction,
    UnsignedTransaction,
};
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

use crate::{
    WalletSdkSpec,
    apis::key_manager::{KeyManagerApi, KeyManagerApiError},
    models::{KeyId, StealthUtxoSpendKeyId},
    spec::KeyStoreError,
    storage::WalletStorageError,
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

    /// Adds one authorization per key in `keys` and `stealth_keys` to `transaction`, under `seal_signer`.
    ///
    /// Every authorization signs over the full authorizer set, so every public key is resolved before anything
    /// is signed. The set is the keys of `keys` and `stealth_keys` together with those of the authorizations
    /// already attached, which must have been made over that same set. The list is kept in canonical order.
    /// A `transaction` that is not [`SchemaVersion::CURRENT`] is refused.
    pub fn authorize(
        &self,
        seal_signer: &RistrettoPublicKeyBytes,
        keys: &[KeyId],
        stealth_keys: &[StealthUtxoSpendKeyId],
        transaction: UnsealedTransaction,
    ) -> Result<UnsealedTransaction, SignerApiError<TSpec>> {
        transaction.schema().ensure_current()?;
        let mut authorizers = AuthorizerSet::from_signatures(transaction.body().signatures());
        for key in keys {
            authorizers.insert(self.key_manager.get_public_key(*key)?.public_key.to_byte_type());
        }
        let stealth_secrets = stealth_keys
            .iter()
            .map(|key| self.key_manager.generate_stealth_owner_key(key))
            .collect::<Result<Vec<_>, _>>()?;
        for secret in &stealth_secrets {
            authorizers.insert(RistrettoPublicKey::from_secret_key(secret).to_byte_type());
        }

        let context = AuthorizationContext::new(seal_signer, &authorizers);
        let (unsigned, mut signatures) = transaction.into_body().into_parts();
        let unsigned = UnsignedTransaction::from_body(SchemaVersion::CURRENT, unsigned);
        for key in keys {
            signatures.push(self.key_manager.sign_with_context(*key, context, &unsigned)?);
        }
        for secret in &stealth_secrets {
            signatures.push(self.key_manager.sign_with_explicit_key(secret, context, &unsigned)?);
        }
        Ok(unsigned.with_signatures(signatures))
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
    #[error(transparent)]
    NotCurrentSchema(#[from] NotCurrentSchema),
}

impl<TSpec: WalletSdkSpec> fmt::Debug for SignerApiError<TSpec> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyStoreError(e) => write!(f, "KeyStoreError: {:?}", e),
            Self::StoreError(e) => write!(f, "StoreError: {:?}", e),
            Self::KeyManagerError(e) => write!(f, "KeyManagerError: {:?}", e),
            Self::NotCurrentSchema(e) => write!(f, "NotCurrentSchema: {:?}", e),
        }
    }
}
