//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use rand::{CryptoRng, Rng};
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_ootle_common_types::Network;
use tari_ootle_transaction::{
    TransactionSealSignature,
    TransactionSignature,
    UnsealedTransactionV1,
    UnsignedTransaction,
};
use tari_ootle_wallet_sdk::OotleAddress;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    signer,
    signer::local_signer::LocalSigner,
    transaction::TransactionSigner,
    wallet::TransactionAuthorization,
    Address,
};

#[derive(Clone)]
pub struct OotleSecretKey {
    account_secret: RistrettoSecretKey,
    view_only_secret: RistrettoSecretKey,
}

impl OotleSecretKey {
    pub fn account_secret(&self) -> &RistrettoSecretKey {
        &self.account_secret
    }

    pub fn view_only_secret(&self) -> &RistrettoSecretKey {
        &self.view_only_secret
    }
}

pub type PrivateKeySigner = LocalSigner<OotleSecretKey>;

impl LocalSigner<OotleSecretKey> {
    pub fn new(network: Network, account_secret: RistrettoSecretKey, view_only_secret: RistrettoSecretKey) -> Self {
        let account_pk = RistrettoPublicKey::from_secret_key(&account_secret);
        let view_only_pk = RistrettoPublicKey::from_secret_key(&view_only_secret);

        let address = OotleAddress::new(network, view_only_pk.to_byte_type(), account_pk.to_byte_type());
        Self {
            address,
            credentials: OotleSecretKey {
                account_secret,
                view_only_secret,
            },
        }
    }

    /// Generate a new PrivateKeySigner with a (non-recoverable) random private key.
    pub fn random(network: Network) -> Self {
        Self::random_with(network, &mut rand::thread_rng())
    }

    pub fn random_with<R: Rng + CryptoRng>(network: Network, rng: &mut R) -> Self {
        let secret = RistrettoSecretKey::random(rng);
        let view_key = RistrettoSecretKey::random(rng);
        Self::new(network, secret, view_key)
    }
}

#[async_trait::async_trait]
impl TransactionSigner for LocalSigner<OotleSecretKey> {
    fn address(&self) -> &Address {
        &self.address
    }

    async fn sign_transaction(&self, message: &UnsealedTransactionV1) -> signer::Result<TransactionSealSignature> {
        Ok(TransactionSealSignature::sign(
            &self.credentials.account_secret,
            message,
        ))
    }

    async fn sign_authorization(
        &self,
        seal_signer: &RistrettoPublicKeyBytes,
        tx: &UnsignedTransaction,
    ) -> signer::Result<TransactionAuthorization> {
        Ok(TransactionSignature::sign(&self.credentials.account_secret, seal_signer, tx).into())
    }
}
