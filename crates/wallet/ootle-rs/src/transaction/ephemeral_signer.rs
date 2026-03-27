//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::{CryptoRng, Rng};
use tari_crypto::{keys::SecretKey, ristretto::RistrettoSecretKey};
use tari_ootle_transaction::{Transaction, UnsealedTransaction};

/// A transaction seal signer that uses an ephemeral secret.
/// WARNING: This signer generates a cryptographically secure secret, signs a transaction and throws the secret away.
/// You probably want to use another implementation e.g. OotleWallet
///
/// This is primarily used in pure stealth transactions where no accounts/components are accessed, no inputs are being
/// spent etc. and thus no specific signature is required.
#[derive(Debug, Clone)]
pub struct EphemeralKeySigner {
    key: RistrettoSecretKey,
}

impl EphemeralKeySigner {
    pub fn random_with<R: Rng + CryptoRng>(rng: &mut R) -> Self {
        let key = RistrettoSecretKey::random(rng);
        Self { key }
    }

    pub fn random() -> Self {
        Self::random_with(&mut rand::thread_rng())
    }

    pub fn seal_transaction(self, transaction: UnsealedTransaction) -> Transaction {
        transaction.seal(&self.key)
    }
}
