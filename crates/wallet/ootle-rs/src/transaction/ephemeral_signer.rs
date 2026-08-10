//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ootle_byte_type::ToByteType;
use rand::{CryptoRng, Rng};
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
};
use tari_ootle_transaction::{Transaction, UnsealedTransaction};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

/// A transaction seal signer that uses an ephemeral secret.
/// WARNING: This signer generates a cryptographically secure secret and keeps it for its own lifetime. Every
/// transaction it seals therefore carries the same seal public key and is linkable to the others, so a signer must not
/// outlive the transaction it seals. You probably want to use another implementation e.g. OotleWallet
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
        Self::random_with(&mut rand::rng())
    }

    /// The public key this signer seals with. An authorization made before the seal must commit to it, so it has to be
    /// available without sealing.
    pub fn public_key(&self) -> RistrettoPublicKeyBytes {
        RistrettoPublicKey::from_secret_key(&self.key).to_byte_type()
    }

    pub fn seal_transaction(&self, transaction: UnsealedTransaction) -> Transaction {
        transaction.seal(&self.key)
    }
}
