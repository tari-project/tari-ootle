//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use blake2::{
    digest::{consts, generic_array::GenericArray},
    Blake2b,
    Digest,
};
use tari_crypto::ristretto::RistrettoSchnorr;
use tari_engine_types::{ConvertFromByteType, FromByteType, ToByteType};
use tari_template_lib_types::crypto::{PublicKey, RistrettoPublicKeyBytes, SignaturePayload};

pub trait GetVerifier {
    fn get_verifier(&self) -> &dyn Verifier;
}

impl GetVerifier for SignaturePayload {
    fn get_verifier(&self) -> &dyn Verifier {
        match self {
            SignaturePayload::RistrettoSchnorrBlake2b(_) => &RistrettoSchnorrBlake2bVerifier,
        }
    }
}

pub trait Verifier {
    fn verify(&self, domain: &[u8], message: &[u8], public_key: &PublicKey, signature: &SignaturePayload) -> bool;
}

/// A verifier for Ristretto Schnorr signatures with Blake2b hashing.
pub struct RistrettoSchnorrBlake2bVerifier;

impl RistrettoSchnorrBlake2bVerifier {
    pub fn compute_challenge(
        domain: &[u8],
        message: &[u8],
        public_key: &RistrettoPublicKeyBytes,
        nonce: &RistrettoPublicKeyBytes,
    ) -> GenericArray<u8, consts::U64> {
        Blake2b::<consts::U64>::new()
            // Domain
            .chain_update(domain)
            // Fiat-Shamir - note that we force this on the user to avoid security pitfalls.
            .chain_update(nonce.as_bytes())
            .chain_update(public_key.as_bytes())
            // Message
            .chain_update(message)
            .finalize()
    }
}

impl Verifier for RistrettoSchnorrBlake2bVerifier {
    fn verify(&self, domain: &[u8], message: &[u8], public_key: &PublicKey, signature: &SignaturePayload) -> bool {
        let sig = signature
            .ristretto_schnorr_blake2b()
            .expect("Expected Ristretto Schnorr signature");
        let ristretto_public_key = public_key.ristretto25519().expect("Expected Ristretto PublicKey");

        let Ok(pk) = ristretto_public_key.try_from_byte_type() else {
            return false;
        };

        if sig.public_nonce().is_zero() {
            return false;
        }

        let Ok(sig) = RistrettoSchnorr::convert_from_byte_type(sig) else {
            return false;
        };

        // NOTE: this is general purpose signature verification, and we want to user to specify a domain, without
        // forcing tari-style domains to be mixed in.
        let message = Self::compute_challenge(
            domain,
            message,
            ristretto_public_key,
            &sig.get_public_nonce().to_byte_type(),
        );
        sig.verify_raw_uniform(&pk, &message)
    }
}

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;
    use tari_crypto::{
        keys::PublicKey as _,
        ristretto::{RistrettoPublicKey, RistrettoSchnorr},
    };
    use tari_engine_types::ToByteType;

    use super::*;

    #[test]
    fn it_verifies_a_valid_signature() {
        let (secret_key, public_key) = RistrettoPublicKey::random_keypair(&mut OsRng);

        let message = b"Hello, world!";
        let domain = b"Test domain";

        let (nonce, public_nonce) = RistrettoPublicKey::random_keypair(&mut OsRng);

        let hashed_message = RistrettoSchnorrBlake2bVerifier::compute_challenge(
            domain,
            message,
            &public_key.to_byte_type(),
            &public_nonce.to_byte_type(),
        );
        let signature = RistrettoSchnorr::sign_raw_uniform(&secret_key, nonce, &hashed_message).unwrap();

        let signature_payload = SignaturePayload::RistrettoSchnorrBlake2b(signature.to_byte_type());
        let public_key = PublicKey::Ristretto25519(public_key.to_byte_type());

        let verifier = RistrettoSchnorrBlake2bVerifier;
        assert!(verifier.verify(domain, message, &public_key, &signature_payload));
    }
}
