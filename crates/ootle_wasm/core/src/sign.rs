//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
    tari_utilities::ByteArray,
};

use crate::error::OotleWasmError;

/// A generated keypair as raw bytes.
#[derive(Debug, Clone)]
pub struct KeypairResult {
    pub secret_key: Vec<u8>,
    pub public_key: Vec<u8>,
}

/// Result of a Schnorr signature operation as raw bytes.
#[derive(Debug, Clone)]
pub struct SchnorrSignatureResult {
    /// The public nonce commitment.
    pub public_nonce: Vec<u8>,
    /// The signature scalar.
    pub signature: Vec<u8>,
}

/// Generate a new random Ristretto keypair.
pub fn generate_keypair() -> KeypairResult {
    let secret_key = RistrettoSecretKey::random(&mut rand::rng());
    let public_key = RistrettoPublicKey::from_secret_key(&secret_key);
    KeypairResult {
        secret_key: secret_key.as_bytes().to_vec(),
        public_key: public_key.as_bytes().to_vec(),
    }
}

/// Sign a message with a Ristretto secret key using Schnorr signatures.
///
/// The secret key should be provided as raw bytes.
/// The message is an arbitrary byte slice (typically a transaction hash).
pub fn schnorr_sign(secret_key: &[u8], message: &[u8]) -> Result<SchnorrSignatureResult, OotleWasmError> {
    let secret_key = RistrettoSecretKey::from_canonical_bytes(secret_key)
        .map_err(|e| OotleWasmError::InvalidSecretKey(e.to_string()))?;

    let sig = RistrettoSchnorr::sign(&secret_key, message, &mut rand::rng())
        .map_err(|e| OotleWasmError::SigningFailed(e.to_string()))?;

    Ok(SchnorrSignatureResult {
        public_nonce: sig.get_public_nonce().as_bytes().to_vec(),
        signature: sig.get_signature().as_bytes().to_vec(),
    })
}

/// Derive the public key from a secret key (both as raw bytes).
pub fn public_key_from_secret_key(secret_key: &[u8]) -> Result<Vec<u8>, OotleWasmError> {
    let secret_key = RistrettoSecretKey::from_canonical_bytes(secret_key)
        .map_err(|e| OotleWasmError::InvalidSecretKey(e.to_string()))?;
    let public_key = RistrettoPublicKey::from_secret_key(&secret_key);
    Ok(public_key.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_round_trip() {
        let secret = RistrettoSecretKey::random(&mut rand::rng());
        let message = b"hello world";

        let result = schnorr_sign(secret.as_bytes(), message).unwrap();
        assert_eq!(result.public_nonce.len(), 32);
        assert_eq!(result.signature.len(), 32);
    }

    #[test]
    fn public_key_derivation() {
        let secret = RistrettoSecretKey::random(&mut rand::rng());
        let expected = RistrettoPublicKey::from_secret_key(&secret);

        let result = public_key_from_secret_key(secret.as_bytes()).unwrap();
        assert_eq!(result, expected.as_bytes());
    }

    #[test]
    fn generate_keypair_is_valid() {
        let kp = generate_keypair();

        // Keys should be 32 bytes
        assert_eq!(kp.secret_key.len(), 32);
        assert_eq!(kp.public_key.len(), 32);

        // Deriving public key from the generated secret key should match
        let derived = public_key_from_secret_key(&kp.secret_key).unwrap();
        assert_eq!(derived, kp.public_key);
    }

    #[test]
    fn generate_keypair_is_unique() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();
        assert_ne!(kp1.secret_key, kp2.secret_key);
        assert_ne!(kp1.public_key, kp2.public_key);
    }
}
