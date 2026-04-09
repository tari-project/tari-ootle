//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey};
use tari_ootle_common_types::engine_types::crypto::OutputBody;
use tari_ootle_transaction::{IntoSigned, Transaction, TransactionSignature, UnsealedTransaction, UnsignedTransaction};
use tari_ootle_wallet_crypto::DecryptedData;
use tari_template_lib_types::{
    Amount,
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes},
    stealth::StealthOutputsStatement,
};

use crate::{
    signer,
    stealth::{Output, SignatureRequirements},
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

    pub async fn decrypt_input_data(
        &self,
        commitment: &PedersenCommitmentBytes,
        input: &OutputBody,
        skip_memo: bool,
    ) -> WalletResult<DecryptedData> {
        let address = self.default_address();
        let signer = self
            .key_providers
            .get(address)
            .ok_or_else(|| WalletError::KeyProviderNotFound {
                address: address.clone(),
            })?;
        let decrypted_data = signer.decrypt_input_data(commitment, input, skip_memo).await?;
        Ok(decrypted_data)
    }

    pub async fn generate_outputs_statement(
        &self,
        specs: Vec<Output>,
        revealed_output_amount: Amount,
    ) -> WalletResult<(StealthOutputsStatement, RistrettoSecretKey)> {
        let address = self.default_address();
        let signer = self
            .key_providers
            .get(address)
            .ok_or_else(|| WalletError::KeyProviderNotFound {
                address: address.clone(),
            })?;
        let (statement, agg_output_mask) = signer.generate_outputs_statement(specs, revealed_output_amount).await?;
        Ok((statement, agg_output_mask))
    }

    pub fn stealth_authorizer(&self, required_signatures: SignatureRequirements) -> WalletStealthAuthorizer<'_, Self> {
        WalletStealthAuthorizer::new(self, required_signatures)
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
