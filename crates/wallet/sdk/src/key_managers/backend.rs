//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey};

use crate::models::{DerivedKeyIndex, KeyId};

pub struct SignatureOutput {
    pub signature: RistrettoSchnorr,
    pub public_key: RistrettoPublicKey,
}

pub trait KeyManagerBackend<M> {
    type Error;

    fn try_sign(&mut self, branch: &str, key_id: KeyId, message: M) -> Result<SignatureOutput, Self::Error>;
}

pub trait WalletKeyStore<K> {
    type Error;

    fn derive_secret(&self, branch: &str, key_index: DerivedKeyIndex) -> Result<RistrettoSecretKey, Self::Error>;

    fn get_imported_secret(&self, key: K) -> Result<RistrettoSecretKey, Self::Error>;
}
