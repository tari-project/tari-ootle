//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey};
use tari_ootle_transaction::{IntoSigned, Transaction, TransactionSignature, UnsealedTransaction, UnsignedTransaction};
use tari_ootle_wallet_crypto::DecryptedData;
use tari_template_lib_types::{
    EncryptedData,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    stealth::StealthTransferStatement,
};

use crate::{
    signer,
    stealth::{BurnClaimStatementSpec, ResolvedStealthTransferSpec, SignatureRequirements},
    transaction::TransactionSealSigner,
    types::Address,
    wallet::{NetworkWallet, WalletStealthAuthorizer, error::WalletError, traits::WalletKeyProvider},
};

pub type WalletResult<T> = Result<T, WalletError>;
type AddressHashMap<T> = HashMap<Address, T>;

/// A wallet that manages multiple key providers and handles transaction signing.
///
/// `OotleWallet` can hold several key providers (each associated with an [`Address`]),
/// with one designated as the default signer. It supports both standard account-key
/// signing and stealth-key signing for confidential transactions.
///
/// Create a wallet from any type implementing [`WalletKeyProvider`]:
///
/// ```rust,ignore
/// let signer = PrivateKeyProvider::random(Network::LocalNet);
/// let mut wallet = OotleWallet::from(signer);
///
/// // Optionally register additional signers
/// let second_signer = PrivateKeyProvider::random(Network::LocalNet);
/// wallet.register_key_provider(second_signer);
/// ```
#[derive(Clone)]
pub struct OotleWallet {
    default: Address,
    key_providers: AddressHashMap<Arc<dyn WalletKeyProvider + Send + Sync>>,
}

impl std::fmt::Debug for OotleWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OotleWallet")
            .field("default", &self.default)
            .field("num_credentials", &self.key_providers.len())
            .finish()
    }
}

impl<S: WalletKeyProvider + Send + Sync + 'static> From<S> for OotleWallet {
    fn from(signer: S) -> Self {
        Self::new(signer)
    }
}

impl OotleWallet {
    /// Create a new wallet with the given signer as the default signer.
    pub fn new<S>(key: S) -> Self
    where S: WalletKeyProvider + Send + Sync + 'static {
        let mut this = Self {
            default: key.address().clone(),
            key_providers: Default::default(),
        };
        this.register_key_provider(key);
        this
    }

    /// Register a new signer on this wallet
    pub fn register_key_provider<K>(&mut self, key: K)
    where K: WalletKeyProvider + Send + Sync + 'static {
        self.key_providers.insert(key.address().clone(), Arc::new(key));
    }

    /// Set the given signer as default.
    /// This signer will be used to sign `TransactionRequest`s.
    pub fn set_default_signer(&mut self, address: &Address) -> WalletResult<()> {
        if self.key_providers.contains_key(address) {
            self.default = address.clone();
            Ok(())
        } else {
            Err(WalletError::KeyProviderNotFound {
                address: address.clone(),
            })
        }
    }

    pub async fn authorize_transaction(
        &self,
        address: &Address,
        unsigned: &UnsignedTransaction,
    ) -> WalletResult<TransactionAuthorization> {
        let default_address = self.default_address();
        let signer = self
            .key_providers
            .get(address)
            .ok_or_else(|| WalletError::KeyProviderNotFound {
                address: address.clone(),
            })?;
        let signature = signer
            .sign_authorization(default_address.account_public_key(), unsigned)
            .await?;
        Ok(signature)
    }

    pub fn additional_signers(&self) -> impl Iterator<Item = &Address> {
        self.key_providers.keys()
    }

    /// Build the complete stealth transfer statement for `spec` using the default key provider.
    /// See [`crate::stealth::StealthStatementProvider::create_transfer_statement`].
    pub async fn create_transfer_statement(
        &self,
        spec: ResolvedStealthTransferSpec,
    ) -> WalletResult<StealthTransferStatement> {
        let address = self.default_address();
        let signer = self
            .key_providers
            .get(address)
            .ok_or_else(|| WalletError::KeyProviderNotFound {
                address: address.clone(),
            })?;
        let statement = signer.create_transfer_statement(spec).await?;
        Ok(statement)
    }

    /// Derive the L1 burn-claim stealth secret `s = H(p·R) + p` using the default key provider.
    /// See [`crate::stealth::BurnClaimKeyProvider::derive_burn_claim_secret`].
    pub async fn derive_burn_claim_secret(
        &self,
        sender_offset_public_key: &RistrettoPublicKey,
    ) -> WalletResult<RistrettoSecretKey> {
        let address = self.default_address();
        let signer = self
            .key_providers
            .get(address)
            .ok_or_else(|| WalletError::KeyProviderNotFound {
                address: address.clone(),
            })?;
        let secret = signer.derive_burn_claim_secret(sender_offset_public_key).await?;
        Ok(secret)
    }

    /// Decrypt an L1 burn output's value and mask using the default key provider.
    /// See [`crate::stealth::BurnClaimKeyProvider::decrypt_burn_claim_output`].
    pub async fn decrypt_burn_claim_output(
        &self,
        encrypted_data: &EncryptedData,
        commitment: &PedersenCommitmentBytes,
        sender_offset_public_key: &RistrettoPublicKey,
    ) -> WalletResult<DecryptedData> {
        let address = self.default_address();
        let signer = self
            .key_providers
            .get(address)
            .ok_or_else(|| WalletError::KeyProviderNotFound {
                address: address.clone(),
            })?;
        let decrypted = signer
            .decrypt_burn_claim_output(encrypted_data, commitment, sender_offset_public_key)
            .await?;
        Ok(decrypted)
    }

    /// Build the statement that spends a minted burn UTXO using the default key provider.
    /// See [`crate::stealth::BurnClaimKeyProvider::create_burn_claim_statement`].
    pub async fn create_burn_claim_statement(
        &self,
        spec: BurnClaimStatementSpec,
    ) -> WalletResult<StealthTransferStatement> {
        let address = self.default_address();
        let signer = self
            .key_providers
            .get(address)
            .ok_or_else(|| WalletError::KeyProviderNotFound {
                address: address.clone(),
            })?;
        let statement = signer.create_burn_claim_statement(spec).await?;
        Ok(statement)
    }

    /// An authorizer for one transaction. Call this per transaction rather than reusing the result: an ephemeral seal
    /// draws its key here, so a shared authorizer seals every transaction with the same key and links them.
    pub fn stealth_authorizer(&self, required_signatures: SignatureRequirements) -> WalletStealthAuthorizer<'_, Self> {
        WalletStealthAuthorizer::new(self, required_signatures)
    }

    /// The one-time public key `address` signs a stealth input with, derived from the input's `public_nonce`.
    /// See [`crate::transaction::TransactionStealthKeySigner::stealth_public_key`].
    pub async fn stealth_public_key(
        &self,
        address: &Address,
        public_nonce: &RistrettoPublicKey,
    ) -> signer::Result<RistrettoPublicKeyBytes> {
        let signer = self.key_providers.get(address).ok_or_else(|| {
            signer::SignerError::other(format!("Signer for address {address} not found in wallet signers"))
        })?;

        signer.stealth_public_key(public_nonce).await
    }

    pub async fn authorize_transaction_with_stealth_key(
        &self,
        address: &Address,
        public_nonce: &RistrettoPublicKey,
        seal_signer: &RistrettoPublicKeyBytes,
        unsigned: &UnsignedTransaction,
    ) -> signer::Result<TransactionAuthorization> {
        let signer = self.key_providers.get(address).ok_or_else(|| {
            signer::SignerError::other(format!("Signer for address {address} not found in wallet signers"))
        })?;

        signer
            .sign_authorization_with_stealth(public_nonce, seal_signer, unsigned)
            .await
    }

    pub async fn seal_transaction_with_stealth_key(
        &self,
        address: &Address,
        public_nonce: &RistrettoPublicKey,
        unsealed: &UnsealedTransaction,
    ) -> signer::Result<Transaction> {
        let signer = self.key_providers.get(address).ok_or_else(|| {
            signer::SignerError::other(format!("Signer for address {address} not found in wallet signers"))
        })?;

        let sig = signer.seal_transaction_with_stealth(public_nonce, unsealed).await?;
        Ok(<UnsealedTransaction as IntoSigned<()>>::into_signed(
            unsealed.clone(),
            sig,
        ))
    }
}

impl NetworkWallet for OotleWallet {
    fn default_address(&self) -> &Address {
        &self.default
    }

    async fn sign_transaction(&self, unsigned: UnsignedTransaction) -> WalletResult<Transaction> {
        let mut signatures = vec![];
        for signer in self.additional_signers() {
            let sig = self.authorize_transaction(signer, &unsigned).await?;
            signatures.push(sig.into_signature());
        }
        let transaction = self.seal_transaction(unsigned.with_signatures(signatures)).await?;
        Ok(transaction)
    }
}

#[async_trait]
impl TransactionSealSigner for OotleWallet {
    async fn seal_transaction(&self, tx: UnsealedTransaction) -> signer::Result<Transaction> {
        let signer = self
            .key_providers
            .get(self.default_address())
            .ok_or_else(|| signer::SignerError::other("Default signer not found in wallet signers"))?;
        let signature = signer.sign_transaction(&tx).await?;
        Ok(<UnsealedTransaction as IntoSigned<()>>::into_signed(tx, signature))
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
