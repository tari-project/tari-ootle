//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::{CryptoRng, Rng};

use crate::{Network, key_provider::local::LocalKeyProvider, keys::OotleSecretKey};

/// A key provider that uses a local OotleSecretKey to sign transactions, decrypt inputs, and derive various stealth
/// secrets.
pub type PrivateKeyProvider = LocalKeyProvider<OotleSecretKey>;
/// A key provider that uses a local OotleSecretKey to sign transactions. This is an alias for PrivateKeyProvider,
/// simply to make the API clearer for alloy-rs users.
pub type PrivateKeySigner = PrivateKeyProvider;

impl LocalKeyProvider<OotleSecretKey> {
    pub fn new(secret: OotleSecretKey) -> Self {
        let address = secret.to_address();
        Self {
            address,
            credentials: secret,
        }
    }

    /// Generate a new PrivateKeySigner with a (non-recoverable) random private key.
    pub fn random(network: Network) -> Self {
        Self::random_with(network, &mut rand::rng())
    }

    pub fn random_with<R: Rng + CryptoRng>(network: Network, rng: &mut R) -> Self {
        let secret = OotleSecretKey::random_with(rng, network);
        Self::new(secret)
    }
}
