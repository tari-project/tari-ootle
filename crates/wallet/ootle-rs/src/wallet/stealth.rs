//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use async_trait::async_trait;
use indexmap::IndexSet;
use tari_ootle_transaction::{Transaction, TransactionSignature, UnsealedTransaction, UnsignedTransaction};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    Address,
    TransactionRequest,
    signer,
    stealth::{SealSource, SignatureRequirements, StealthSignerRequirement},
    transaction::{TransactionSealSigner, ephemeral_signer::EphemeralKeySigner},
    wallet::{NetworkWallet, OotleWallet, TransactionAuthorization, WalletResult},
};

/// The seal key this authorizer will use, resolved from a [`SealSource`].
///
/// The ephemeral key is drawn when the authorizer is constructed rather than when the transaction is sealed, so that
/// an authorization taken from [`WalletStealthAuthorizer::authorization_message`] commits to the key that actually
/// ends up sealing.
#[allow(clippy::large_enum_variant)]
enum Seal {
    AccountKey,
    StealthInput(StealthSignerRequirement),
    Ephemeral(EphemeralKeySigner),
}

impl From<SealSource> for Seal {
    fn from(source: SealSource) -> Self {
        match source {
            SealSource::AccountKey => Self::AccountKey,
            SealSource::StealthInput(signer) => Self::StealthInput(signer),
            SealSource::Ephemeral => Self::Ephemeral(EphemeralKeySigner::random()),
        }
    }
}

/// Seals a stealth transaction and produces the stealth authorization signatures its inputs require.
///
/// Both halves live here because an authorization signs a message committing to the seal signer's public key: only
/// whoever decides the seal key can produce them, and they must be attached before the seal signature is made over
/// them.
///
/// Build one authorizer per transaction. An ephemeral seal draws its key once, when the authorizer is constructed, so
/// reusing an authorizer seals every transaction with the same key and links them to each other.
pub struct WalletStealthAuthorizer<'a, W: ?Sized> {
    wallet: &'a W,
    seal: Seal,
    authorizers: IndexSet<StealthSignerRequirement>,
}

impl<'a, W: ?Sized> WalletStealthAuthorizer<'a, W> {
    pub fn new(wallet: &'a W, required_signatures: SignatureRequirements) -> Self {
        let (seal, authorizers) = required_signatures.into_parts();
        Self {
            wallet,
            seal: seal.into(),
            authorizers,
        }
    }
}

impl WalletStealthAuthorizer<'_, OotleWallet> {
    /// The public key that seals the transaction. A stealth seal derives the input owner's one-time key, so this
    /// consults the owning key provider.
    pub async fn seal_public_key(&self) -> signer::Result<RistrettoPublicKeyBytes> {
        match &self.seal {
            Seal::AccountKey => Ok(*self.wallet.default_address().account_public_key()),
            Seal::StealthInput(signer) => {
                self.wallet
                    .stealth_public_key(signer.signer(), signer.public_nonce())
                    .await
            },
            Seal::Ephemeral(signer) => Ok(signer.public_key()),
        }
    }

    /// The exact message every authorization signature for this transaction must sign. An external co-signer (or an
    /// adaptor pre-signer) produces its signature over these bytes; the resulting [`TransactionAuthorization`] is
    /// attached via [`crate::TransactionRequest::add_authorization`].
    pub async fn authorization_message(&self, unsigned: &UnsignedTransaction) -> signer::Result<[u8; 64]> {
        let seal_signer = self.seal_public_key().await?;
        let UnsignedTransaction::V1(unsigned_v1) = unsigned;
        Ok(TransactionSignature::create_message_v1(&seal_signer, unsigned_v1))
    }
}

#[async_trait]
impl TransactionSealSigner for WalletStealthAuthorizer<'_, OotleWallet> {
    async fn authorizations_for(
        &self,
        unsigned: &UnsignedTransaction,
    ) -> signer::Result<Vec<TransactionAuthorization>> {
        if self.authorizers.is_empty() {
            return Ok(Vec::new());
        }

        let seal_signer = self.seal_public_key().await?;
        let mut authorizations = Vec::with_capacity(self.authorizers.len());
        for req in &self.authorizers {
            let auth = self
                .wallet
                .authorize_transaction_with_stealth_key(req.signer(), req.public_nonce(), &seal_signer, unsigned)
                .await?;
            authorizations.push(auth);
        }
        Ok(authorizations)
    }

    async fn seal_transaction(&self, transaction: UnsealedTransaction) -> signer::Result<Transaction> {
        match &self.seal {
            Seal::AccountKey => self.wallet.seal_transaction(transaction).await,
            Seal::StealthInput(signer) => {
                self.wallet
                    .seal_transaction_with_stealth_key(signer.signer(), signer.public_nonce(), &transaction)
                    .await
            },
            Seal::Ephemeral(signer) => Ok(signer.seal_transaction(transaction)),
        }
    }
}

impl NetworkWallet for WalletStealthAuthorizer<'_, OotleWallet> {
    fn default_address(&self) -> &Address {
        self.wallet.default_address()
    }

    async fn sign_transaction(&self, unsigned: UnsignedTransaction) -> WalletResult<Transaction> {
        TransactionRequest::new().with_transaction(unsigned).build(self).await
    }
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    };

    use super::*;
    use crate::{
        Network,
        key_provider::PrivateKeyProvider,
        transaction::{TransactionSigner, adaptor::sign_authorization},
    };

    /// A distinct sender public nonce, standing in for the one recorded on a spent UTXO.
    fn public_nonce(seed: u8) -> RistrettoPublicKey {
        let secret = RistrettoSecretKey::from_uniform_bytes(&[seed; 64]).expect("seed is the right length");
        RistrettoPublicKey::from_secret_key(&secret)
    }

    fn wallet() -> (OotleWallet, Address) {
        let provider = PrivateKeyProvider::random(Network::LocalNet);
        let address = provider.address().clone();
        (OotleWallet::from(provider), address)
    }

    /// The stealth inputs `seeds` describes, all owned by `address`.
    fn signers<const N: usize>(address: &Address, seeds: [u8; N]) -> IndexSet<StealthSignerRequirement> {
        seeds
            .into_iter()
            .map(|seed| StealthSignerRequirement::new(address.clone(), public_nonce(seed)))
            .collect()
    }

    fn unsigned() -> UnsignedTransaction {
        Transaction::builder(Network::LocalNet).build_unsigned()
    }

    /// Several stealth inputs owned by the same address: one seals with its one-time key and the rest authorize
    /// against it. Every authorization must verify under the key that actually sealed.
    #[tokio::test]
    async fn stealth_authorizations_verify_against_the_sealing_one_time_key() {
        let (wallet, address) = wallet();
        let authorizer = wallet.stealth_authorizer(SignatureRequirements::stealth_seal(signers(&address, [1, 2, 3])));

        let transaction = TransactionRequest::new()
            .with_transaction(unsigned())
            .build(&authorizer)
            .await
            .expect("sealing a multi-input stealth transfer must succeed");

        // The two inputs that did not seal each contribute an authorization.
        assert_eq!(transaction.signatures().len(), 2);
        assert!(transaction.verify_all_signatures());

        // The seal is the first input's one-time key, never the account key it derives from.
        let expected_seal = wallet
            .stealth_public_key(&address, &public_nonce(1))
            .await
            .expect("the wallet owns the sealing input");
        assert_eq!(transaction.seal_signature().public_key(), &expected_seal);
        assert_ne!(transaction.seal_signature().public_key(), address.account_public_key());
    }

    /// A single stealth input seals and authorizes nothing: the seal signature is its own spend authorization.
    #[tokio::test]
    async fn a_single_stealth_input_seals_without_authorizations() {
        let (wallet, address) = wallet();
        let authorizer = wallet.stealth_authorizer(SignatureRequirements::stealth_seal(signers(&address, [1])));

        let transaction = TransactionRequest::new()
            .with_transaction(unsigned())
            .build(&authorizer)
            .await
            .expect("sealing a single-input stealth transfer must succeed");

        assert!(transaction.signatures().is_empty());
        assert!(transaction.verify_all_signatures());
    }

    /// When the account component is accessed the account key seals, and every stealth input authorizes against it.
    #[tokio::test]
    async fn account_key_seal_authorizes_every_stealth_input() {
        let (wallet, address) = wallet();
        let authorizer =
            wallet.stealth_authorizer(SignatureRequirements::account_key_seal_with(signers(&address, [1, 2])));

        let transaction = TransactionRequest::new()
            .with_transaction(unsigned())
            .build(&authorizer)
            .await
            .expect("sealing with the account key must succeed");

        assert_eq!(transaction.signatures().len(), 2);
        assert!(transaction.verify_all_signatures());
        assert_eq!(transaction.seal_signature().public_key(), address.account_public_key());
    }

    /// An externally produced authorization signs `authorization_message`, so the key that message names must be the
    /// key that ends up sealing — including the ephemeral one, which is drawn before the message is handed out.
    #[tokio::test]
    async fn an_external_authorization_verifies_against_every_seal_source() {
        let (wallet, address) = wallet();
        let requirements = [
            SignatureRequirements::stealth_seal(signers(&address, [1, 2])),
            SignatureRequirements::account_key_seal(),
            SignatureRequirements::stealth_seal(IndexSet::new()),
        ];

        for requirement in requirements {
            let authorizer = wallet.stealth_authorizer(requirement);
            let unsigned = unsigned();

            let secret = RistrettoSecretKey::random(&mut rand::rng());
            let message = authorizer
                .authorization_message(&unsigned)
                .await
                .expect("the seal key is resolvable before sealing");

            let transaction = TransactionRequest::new()
                .with_transaction(unsigned)
                .add_authorization(sign_authorization(&secret, &message))
                .build(&authorizer)
                .await
                .expect("sealing must succeed");

            assert!(transaction.verify_all_signatures());
            assert!(
                transaction
                    .signatures()
                    .iter()
                    .any(|sig| sig.public_key() == &RistrettoPublicKey::from_secret_key(&secret).to_byte_type()),
                "the external authorization must be attached"
            );
        }
    }

    /// The ephemeral seal key is drawn per authorizer and is neither the account key nor reused.
    #[tokio::test]
    async fn an_ephemeral_seal_uses_a_fresh_key() {
        let (wallet, address) = wallet();

        let first = wallet
            .stealth_authorizer(SignatureRequirements::stealth_seal(IndexSet::new()))
            .seal_public_key()
            .await
            .expect("an ephemeral key needs no key provider");
        let second = wallet
            .stealth_authorizer(SignatureRequirements::stealth_seal(IndexSet::new()))
            .seal_public_key()
            .await
            .expect("an ephemeral key needs no key provider");

        assert_ne!(first, second);
        assert_ne!(&first, address.account_public_key());
    }
}
