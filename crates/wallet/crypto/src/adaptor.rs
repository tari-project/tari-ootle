//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Schnorr adaptor signatures over Ristretto ("scriptless scripts").
//!
//! An adaptor signature is a Schnorr signature "encrypted" under an adaptor point `T = t·G`: a
//! valid-looking pre-signature anyone can verify, but which only the holder of the discrete log `t`
//! can complete into a full signature. Completing it reveals `t` (recoverable by subtracting the
//! pre-signature scalar from the completed one), which is what makes a cross-chain atomic swap
//! atomic — completing one chain's signature publishes the secret that unlocks the other.
//!
//! The completed signature is an ordinary [`RistrettoSchnorr`] whose public nonce is `R + T`, so it
//! verifies with the stock verifier and needs no consensus change. The challenge is built with
//! tari_crypto's own [`RistrettoSchnorr::construct_domain_separated_challenge`] over the **offset**
//! nonce `R + T`, so a completed adaptor signature is byte-for-byte one the standard signer could
//! have produced over that nonce.

use blake2::Blake2b;
use digest::consts::U64;
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSchnorr, RistrettoSecretKey},
};

/// A Schnorr adaptor pre-signature encrypted under an adaptor point `T = t·G`.
///
/// `public_nonce` is the offset nonce `R + T` that the completed signature will carry; `signature`
/// is the pre-signature scalar `ŝ = r + e·x`, which does **not** include `t`. Completing with `t`
/// yields `RistrettoSchnorr::new(R + T, ŝ + t)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RistrettoAdaptorSignature {
    public_nonce: RistrettoPublicKey,
    signature: RistrettoSecretKey,
}

impl RistrettoAdaptorSignature {
    /// The offset public nonce `R + T` the completed signature will carry.
    pub fn public_nonce(&self) -> &RistrettoPublicKey {
        &self.public_nonce
    }

    /// The pre-signature scalar `ŝ = r + e·x`.
    pub fn signature(&self) -> &RistrettoSecretKey {
        &self.signature
    }
}

/// The challenge scalar the stock signer/verifier uses: `from_uniform_bytes(H(public_nonce ‖ public_key ‖ message))`,
/// where `H` is tari_crypto's domain-separated `Blake2b<U64>` challenge. Reusing the library's own construction is
/// what guarantees a completed adaptor signature verifies against the unchanged verifier.
fn challenge(public_nonce: &RistrettoPublicKey, public_key: &RistrettoPublicKey, message: &[u8]) -> RistrettoSecretKey {
    let hash =
        RistrettoSchnorr::construct_domain_separated_challenge::<_, Blake2b<U64>>(public_nonce, public_key, message);
    RistrettoSecretKey::from_uniform_bytes(hash.as_ref()).expect("Blake2b<U64> yields 64 uniform bytes")
}

/// Produces an adaptor pre-signature `ŝ = r + e·x` with `e = H((R + T) ‖ P ‖ message)`, where
/// `R = r·G` for a freshly sampled nonce `r`, `P = secret·G`, and `T = adaptor_point`. The challenge
/// binds the **offset** nonce `R + T` so [`complete`] yields a signature the stock verifier accepts.
///
/// The nonce is drawn internally from the OS CSPRNG on each call, so it cannot be reused across
/// signatures (nonce reuse across two messages would leak `secret`).
pub fn sign(
    secret: &RistrettoSecretKey,
    adaptor_point: &RistrettoPublicKey,
    message: &[u8],
) -> RistrettoAdaptorSignature {
    let nonce = RistrettoSecretKey::random(&mut rand::rng());
    let public_key = RistrettoPublicKey::from_secret_key(secret);
    let r = RistrettoPublicKey::from_secret_key(&nonce);
    let public_nonce = &r + adaptor_point; // R + T
    let e = challenge(&public_nonce, &public_key, message);
    let ek = &e * secret;
    let s_hat = &nonce + &ek; // ŝ = r + e·x
    RistrettoAdaptorSignature {
        public_nonce,
        signature: s_hat,
    }
}

/// Verifies an adaptor pre-signature against `public_key`, `adaptor_point` (`T`) and `message`,
/// checking `ŝ·G + T == (R + T) + e·P`. A `true` result guarantees that [`complete`] with
/// the discrete log of `T` produces a signature valid under the stock verifier.
pub fn verify(
    pre: &RistrettoAdaptorSignature,
    public_key: &RistrettoPublicKey,
    adaptor_point: &RistrettoPublicKey,
    message: &[u8],
) -> bool {
    if public_key == &RistrettoPublicKey::default() {
        return false;
    }
    let e = challenge(&pre.public_nonce, public_key, message);
    let s_hat_g = RistrettoPublicKey::from_secret_key(&pre.signature);
    let lhs = &s_hat_g + adaptor_point;
    let rhs = &pre.public_nonce + &e * public_key;
    lhs == rhs
}

/// Completes the pre-signature with the adaptor secret `t` (`T = t·G`), yielding an ordinary
/// signature `(R + T, ŝ + t)` that verifies with the stock [`RistrettoSchnorr`] verifier.
///
/// The caller is responsible for ensuring `t` is the discrete log of the adaptor point the
/// pre-signature was produced under; verify with [`verify`] first if in doubt.
pub fn complete(pre: &RistrettoAdaptorSignature, t: &RistrettoSecretKey) -> RistrettoSchnorr {
    let s = &pre.signature + t;
    RistrettoSchnorr::new(pre.public_nonce.clone(), s)
}

/// Recovers the adaptor secret `t` from a pre-signature and its completed signature: `t = s − ŝ`.
///
/// This is the swap-reveal primitive: a watcher who holds the pre-signature and observes the
/// completed signature on-chain recovers the secret that unlocks the other leg.
pub fn extract(pre: &RistrettoAdaptorSignature, completed: &RistrettoSchnorr) -> RistrettoSecretKey {
    completed.get_signature() - &pre.signature
}

#[cfg(test)]
mod tests {
    use tari_crypto::ristretto::{RistrettoPublicKey, RistrettoSecretKey};

    use super::*;

    fn keypair() -> (RistrettoSecretKey, RistrettoPublicKey) {
        let secret = RistrettoSecretKey::random(&mut rand::rng());
        let public = RistrettoPublicKey::from_secret_key(&secret);
        (secret, public)
    }

    const MESSAGE: &[u8] = b"adaptor signature test message";

    #[test]
    fn complete_yields_a_signature_the_stock_verifier_accepts() {
        let (x, p) = keypair();
        let (t, big_t) = keypair();
        let pre = sign(&x, &big_t, MESSAGE);
        assert!(verify(&pre, &p, &big_t, MESSAGE));

        let sig = complete(&pre, &t);
        // The completed signature is an ordinary RistrettoSchnorr with no adaptor awareness needed.
        assert!(sig.verify(&p, MESSAGE));
    }

    #[test]
    fn extract_recovers_the_adaptor_secret() {
        let (x, _p) = keypair();
        let (t, big_t) = keypair();
        let pre = sign(&x, &big_t, MESSAGE);
        let sig = complete(&pre, &t);

        assert_eq!(extract(&pre, &sig), t);
    }

    #[test]
    fn completing_with_the_wrong_secret_does_not_verify() {
        let (x, p) = keypair();
        let (_t, big_t) = keypair();
        let (wrong_t, _) = keypair();
        let pre = sign(&x, &big_t, MESSAGE);
        let sig = complete(&pre, &wrong_t);
        assert!(!sig.verify(&p, MESSAGE));
    }

    #[test]
    fn verify_rejects_a_tampered_message_or_wrong_key() {
        let (x, p) = keypair();
        let (_t, big_t) = keypair();
        let (_, other_p) = keypair();
        let pre = sign(&x, &big_t, MESSAGE);
        assert!(!verify(&pre, &p, &big_t, b"different message"));
        assert!(!verify(&pre, &other_p, &big_t, MESSAGE));
    }
}
