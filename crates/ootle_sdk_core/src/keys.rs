//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The explicit key material the public-transfer path consumes.
//!
//! The core is a pure function of its inputs, so every key is supplied explicitly by the caller, and
//! all signing-nonce randomness is expanded from a single caller-supplied seed (or drawn fresh from
//! the OS RNG). Nothing here ever reaches for an RNG: the random seal path (which does use `OsRng`)
//! lives in [`crate::tx`] and only borrows the secret key from here.
//!
//! Two bundles are exposed:
//!
//! - [`PublicTransferKeys`] — the account secret key alone. The random seal path expands a fresh OS-RNG seed, so the
//!   resulting bytes are **not** reproducible (this is fine for real submission, where uniqueness is desirable).
//! - [`DeterministicTransferKeys`] — the account secret key plus a 32-byte build seed. The seal path expands the seed
//!   into the pinned authorization + seal nonces, making the encoded bytes and the transaction id byte-for-byte
//!   reproducible from the seed.
//!
//! Single-key public transfer: the account key is simultaneously the sole authorization signer and
//! the seal signer (the reproducible path expands its two nonces from the seed). The API also supports
//! a *separate* seal signer for completeness.

use tari_crypto::{
    keys::PublicKey as _,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::types::{
    bytes::{BuildSeed, NonceSecretBytes, SecretKeyBytes},
    error::OotleSdkError,
};

/// Parses a boundary secret key into the internal [`RistrettoSecretKey`].
///
/// Maps a malformed key to [`OotleSdkError::Key`] rather than panicking.
pub(crate) fn parse_secret_key(secret: &SecretKeyBytes) -> Result<RistrettoSecretKey, OotleSdkError> {
    RistrettoSecretKey::from_canonical_bytes(secret.as_bytes())
        .map_err(|e| OotleSdkError::Key(format!("invalid secret key: {e}")))
}

/// Parses a boundary nonce secret into the internal [`RistrettoSecretKey`] (a Ristretto scalar).
///
/// A nonce secret is the same shape as a secret key (a 32-byte canonical scalar); it just plays a
/// different role in [`tari_crypto::ristretto::RistrettoSchnorr::sign_with_nonce_and_message`].
pub(crate) fn parse_nonce_secret(nonce: &NonceSecretBytes) -> Result<RistrettoSecretKey, OotleSdkError> {
    RistrettoSecretKey::from_canonical_bytes(nonce.as_bytes())
        .map_err(|e| OotleSdkError::Key(format!("invalid nonce secret: {e}")))
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
/// the seal signer (single-key transfer). The seal expands a fresh OS-RNG seed for its nonces, so the
/// encoded bytes are not reproducible — use [`DeterministicTransferKeys`] for the seed-reproducible path.
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

/// The seed-reproducible key bundle.
///
/// Carries the account secret key plus a 32-byte build seed. The seal path expands the seed into the
/// pinned authorization + seal nonces (distinct, non-zero, canonical scalars), so the two signatures —
/// and therefore the encoded bytes and the transaction id — are reproducible from the seed.
#[derive(Debug, Clone)]
pub struct DeterministicTransferKeys {
    /// The account secret key (authorization signer **and** seal signer for the single-key path).
    pub account_secret: SecretKeyBytes,
    /// The 32-byte build seed the seal expands into the pinned authorization + seal nonces.
    pub seed: BuildSeed,
    /// An optional separate seal secret key. When `None`, the account key is the seal signer
    /// (single-key transfer). When `Some`, this key seals instead.
    pub seal_secret: Option<SecretKeyBytes>,
}

impl DeterministicTransferKeys {
    /// Builds the single-key bundle (account key is also the seal signer).
    pub fn single_key(account_secret: SecretKeyBytes, seed: BuildSeed) -> Self {
        Self {
            account_secret,
            seed,
            seal_secret: None,
        }
    }

    /// Builds the separate-signer bundle (a distinct seal key).
    pub fn separate_signer(account_secret: SecretKeyBytes, seal_secret: SecretKeyBytes, seed: BuildSeed) -> Self {
        Self {
            account_secret,
            seed,
            seal_secret: Some(seal_secret),
        }
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

    #[test]
    fn nonce_secret_parses_like_a_key() {
        let mut b = [0u8; 32];
        b[0] = 3;
        let nonce = NonceSecretBytes::from_array(b);
        assert!(parse_nonce_secret(&nonce).is_ok());
    }
}
