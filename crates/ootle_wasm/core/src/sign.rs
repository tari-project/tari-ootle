//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use rand::rngs::OsRng;
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
    tari_utilities::ByteArray,
};

use crate::error::OotleWasmError;

/// A generated keypair with hex-encoded keys.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeypairResult {
    pub secret_key: String,
    pub public_key: String,
}

/// Result of a Schnorr signature operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SchnorrSignatureResult {
    /// The public nonce commitment (hex-encoded).
    pub public_nonce: String,
    /// The signature scalar (hex-encoded).
    pub signature: String,
}

/// Generate a new random Ristretto keypair.
pub fn generate_keypair() -> KeypairResult {
    let secret_key = RistrettoSecretKey::random(&mut OsRng);
    let public_key = RistrettoPublicKey::from_secret_key(&secret_key);
    KeypairResult {
        secret_key: hex::encode(secret_key.as_bytes()),
        public_key: hex::encode(public_key.as_bytes()),
    }
}

/// Sign a message with a Ristretto secret key using Schnorr signatures.
///
/// The secret key should be provided as a hex string.
/// The message is an arbitrary byte slice (typically a transaction hash).
pub fn schnorr_sign(secret_key_hex: &str, message: &[u8]) -> Result<SchnorrSignatureResult, OotleWasmError> {
    let secret_key = secret_key_from_hex(secret_key_hex)?;

    let sig = RistrettoSchnorr::sign(&secret_key, message, &mut OsRng)
        .map_err(|e| OotleWasmError::SigningFailed(e.to_string()))?;

    Ok(SchnorrSignatureResult {
        public_nonce: hex::encode(sig.get_public_nonce().as_bytes()),
        signature: hex::encode(sig.get_signature().as_bytes()),
    })
}

fn secret_key_from_hex(hex_str: &str) -> Result<RistrettoSecretKey, OotleWasmError> {
    let bytes = hex::decode(hex_str)?;
    RistrettoSecretKey::from_canonical_bytes(&bytes).map_err(|e| OotleWasmError::InvalidSecretKey(e.to_string()))
}

/// Derive the public key from a secret key (both hex-encoded).
pub fn public_key_from_secret_key(secret_key_hex: &str) -> Result<String, OotleWasmError> {
    use tari_crypto::keys::PublicKey;
    let secret_key = secret_key_from_hex(secret_key_hex)?;
    let public_key = RistrettoPublicKey::from_secret_key(&secret_key);
    Ok(hex::encode(public_key.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_round_trip() {
        let secret = RistrettoSecretKey::random(&mut OsRng);
        let secret_hex = hex::encode(secret.as_bytes());
        let message = b"hello world";

        let result = schnorr_sign(&secret_hex, message).unwrap();
        assert!(!result.public_nonce.is_empty());
        assert!(!result.signature.is_empty());
    }

    #[test]
    fn public_key_derivation() {
        let secret = RistrettoSecretKey::random(&mut OsRng);
        let secret_hex = hex::encode(secret.as_bytes());
        let expected = RistrettoPublicKey::from_secret_key(&secret);

        let result = public_key_from_secret_key(&secret_hex).unwrap();
        assert_eq!(result, hex::encode(expected.as_bytes()));
    }

    #[test]
    fn generate_keypair_is_valid() {
        let kp = generate_keypair();

        // Keys should be 32 bytes = 64 hex chars
        assert_eq!(kp.secret_key.len(), 64);
        assert_eq!(kp.public_key.len(), 64);

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
