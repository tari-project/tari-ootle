//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The explicit key material the public-transfer path consumes.
//!
//! The core is a pure function of its inputs, so every key is supplied explicitly by the caller.
//! Signing draws a fresh nonce from the OS RNG in [`crate::tx`] (this module only borrows the secret
//! key from here); nothing here ever reaches for an RNG.
//!
//! One bundle is exposed:
//!
//! - [`PublicTransferKeys`] — the account secret key alone. The seal signs with a fresh OS-RNG nonce, so the resulting
//!   bytes are **not** reproducible (this is fine for real submission, where uniqueness is desirable). A single-key
//!   public transfer uses the account key as both the sole authorization signer and the seal signer.

use tari_crypto::{
    keys::PublicKey as _,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::types::{bytes::SecretKeyBytes, error::OotleSdkError};

/// Parses a boundary secret key into the internal [`RistrettoSecretKey`].
///
/// Maps a malformed key to [`OotleSdkError::Key`] rather than panicking.
pub(crate) fn parse_secret_key(secret: &SecretKeyBytes) -> Result<RistrettoSecretKey, OotleSdkError> {
    RistrettoSecretKey::from_canonical_bytes(secret.as_bytes())
        .map_err(|e| OotleSdkError::Key(format!("invalid secret key: {e}")))
}

/// Derives the internal byte-typed public key from a secret key (deterministic).
pub(crate) fn public_key_bytes_from_secret(secret: &RistrettoSecretKey) -> RistrettoPublicKeyBytes {
    let pk = RistrettoPublicKey::from_secret_key(secret);
    // RistrettoPublicKey is always 32 bytes; this width never mismatches.
    RistrettoPublicKeyBytes::from_bytes(pk.as_bytes())
        .expect("RistrettoPublicKey is always 32 bytes — width is guaranteed")
}

/// The account-secret-only key bundle for a public transfer.
///
/// Carries only the account secret key. The account key is both the sole authorization signer and
/// the seal signer (single-key transfer). The seal signs with a fresh OS-RNG nonce, so the encoded
/// bytes are not reproducible.
#[derive(Debug, Clone)]
pub struct PublicTransferKeys {
    /// The account secret key (signs the authorization signature and seals the transaction).
    pub account_secret: SecretKeyBytes,
}

impl PublicTransferKeys {
    /// Builds the account-secret-only bundle.
    pub fn new(account_secret: SecretKeyBytes) -> Self {
        Self { account_secret }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_secret() -> SecretKeyBytes {
        // A fixed, canonical Ristretto scalar (low value → always canonical).
        let mut b = [0u8; 32];
        b[0] = 7;
        SecretKeyBytes::from_array(b)
    }

    #[test]
    fn parses_canonical_secret_key() {
        let sk = parse_secret_key(&sample_secret()).unwrap();
        let pk = public_key_bytes_from_secret(&sk);
        assert_eq!(pk.as_bytes().len(), 32);
    }

    #[test]
    fn rejects_non_canonical_secret_key() {
        // All-0xff is not a canonical Ristretto scalar.
        let bad = SecretKeyBytes::from_array([0xff; 32]);
        let err = parse_secret_key(&bad).unwrap_err();
        assert_eq!(err.code(), "KEY");
    }
}
