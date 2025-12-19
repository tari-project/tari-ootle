//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::rngs::OsRng;
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};
use tari_ootle_common_types::Signable;

use crate::models::DerivedKeyIndex;

pub struct SignatureOutput {
    pub signature: RistrettoSchnorr,
    pub public_key: RistrettoPublicKey,
}

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
        let signature = RistrettoSchnorr::sign(&secret, message.to_signing_message(context), &mut OsRng)
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
