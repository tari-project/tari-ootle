//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_common_types::seeds::cipher_seed::CipherSeed;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_ootle_common_types::signature::SignatureOutput;
use tari_ootle_transaction::Signable;
use tari_ootle_wallet_crypto::derive_ristretto_key;

use crate::models::DerivedKeyIndex;

pub trait WalletKeyStore {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Derive a secret key for the given branch and key index.
    fn derive_secret(&self, branch: &str, key_index: DerivedKeyIndex) -> Result<RistrettoSecretKey, Self::Error>;

    /// Sign a message using a derived key for the given branch and key index.
    fn sign<M: Signable<C>, C>(
        &self,
        key_branch: &str,
        index: DerivedKeyIndex,
        context: C,
        message: &M,
    ) -> Result<SignatureOutput, Self::Error> {
        let secret = self.derive_secret(key_branch, index)?;
        let signature = RistrettoSchnorr::sign(&secret, message.to_signing_message(context), &mut rand::rng())
            .expect("RistrettoSchnorr::sign is infallible as it internally hashes the message into canonical form");
        Ok(SignatureOutput {
            signature,
            public_key: RistrettoPublicKey::from_secret_key(&secret),
        })
    }

    /// Retrieve the key birthday if it exists. The birthday is defined as the number of seconds since the zero epoch,
    /// which is predefined for a given network. If this is not supported, it is correct to return Ok(None).
    fn key_birthday(&self) -> Result<Option<u16>, Self::Error>;
}

pub trait WithCipherSeed {
    type Error: std::error::Error + Send + Sync + 'static;
    fn get_cipher_seed(&self) -> Result<&CipherSeed, Self::Error>;
}

impl WithCipherSeed for CipherSeed {
    type Error = std::convert::Infallible;

    fn get_cipher_seed(&self) -> Result<&CipherSeed, Self::Error> {
        Ok(self)
    }
}

impl<T: WithCipherSeed> WalletKeyStore for T {
    type Error = T::Error;

    fn derive_secret(&self, branch: &str, key_index: DerivedKeyIndex) -> Result<RistrettoSecretKey, Self::Error> {
        let cipher_seed = self.get_cipher_seed()?;
        let secret = derive_ristretto_key(cipher_seed.entropy(), branch.as_bytes(), key_index);
        Ok(secret)
    }

    fn key_birthday(&self) -> Result<Option<u16>, Self::Error> {
        let seed = self.get_cipher_seed()?;
        Ok(Some(seed.birthday()))
    }
}
