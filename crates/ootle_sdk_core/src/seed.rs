//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Seed expansion: derive every per-output construction scalar a stealth build consumes from one
//! 32-byte [`BuildSeed`].
//!
//! A caller supplies a single seed; this module expands it, via a domain-separated KDF, into the
//! distinct scalars each output's construction needs (the commitment mask, the sender ephemeral nonce,
//! the AEAD nonce, and the ElGamal/ZK nonces for a viewable-balance proof). Same seed + same intent ⇒
//! same scalars, so a build's *construction* is reproducible across machines and languages, while
//! cross-output reuse is structurally impossible (each derived scalar is bound to its field label and
//! its output index).
//!
//! Signatures are **not** derived here: every authorization/seal/cosign signature is signed with a
//! fresh random nonce (see [`crate::tx`] / [`crate::cosign`] / [`crate::stealth::sign_seal`]). A seed
//! never feeds a Schnorr nonce.
//!
//! # The `v1` derivation contract
//!
//! The domain string, the per-field labels, and the input order (`domain ‖ label ‖ seed ‖ index`)
//! are a frozen cross-language reproducibility contract: every golden vector and every host
//! re-implementation depends on these exact bytes. Changing any of them is a vector-invalidating
//! `v2`, not an edit.

use blake2::{Blake2b, Digest, digest::consts::U64};
use tari_crypto::{keys::SecretKey, ristretto::RistrettoSecretKey, tari_utilities::ByteArray};

use crate::types::bytes::{BuildSeed, SecretKeyBytes};

/// Hash domain for seed expansion. The `v1` suffix is the frozen contract version: changing it (or
/// any label below, or the input order) invalidates every golden vector.
const DOMAIN: &[u8] = b"ootle.sdk.entropy.v1";

// Frozen per-field labels — the cross-language derivation contract. Never change these strings or the
// input order once shipped; a change is a `v2`.
const LABEL_MASK: &[u8] = b"mask";
const LABEL_SENDER_NONCE: &[u8] = b"sender-nonce";
const LABEL_AEAD_NONCE: &[u8] = b"aead-nonce";
const LABEL_ELGAMAL_NONCE: &[u8] = b"elgamal-nonce";
const LABEL_ZK_XV: &[u8] = b"zk-xv";
const LABEL_ZK_XM: &[u8] = b"zk-xm";
const LABEL_ZK_XR: &[u8] = b"zk-xr";

/// The single derivation primitive: a domain-separated Blake2b-512 hash of
/// `DOMAIN ‖ label ‖ seed ‖ (index as u64 LE)` reduced to a canonical Ristretto scalar.
///
/// `index` binds the scalar to output `i`, which is what makes cross-output reuse structurally
/// impossible. The 64-byte digest is reduced via [`RistrettoSecretKey::from_uniform_bytes`], so the
/// result is always a non-zero canonical scalar.
fn derive_scalar(seed: &BuildSeed, label: &[u8], index: u64) -> SecretKeyBytes {
    let mut hasher = Blake2b::<U64>::new();
    hasher.update(DOMAIN);
    hasher.update(label);
    hasher.update(seed.as_bytes());
    hasher.update(index.to_le_bytes());
    let wide: [u8; 64] = hasher.finalize().into();
    let sk = RistrettoSecretKey::from_uniform_bytes(&wide).expect("64 uniform bytes reduce to a canonical scalar");
    SecretKeyBytes::from_bytes(sk.as_bytes()).expect("RistrettoSecretKey is always 32 bytes")
}

/// Derives a per-output mask scalar bound to output `index`.
pub(crate) fn derive_mask(seed: &BuildSeed, index: u64) -> SecretKeyBytes {
    derive_scalar(seed, LABEL_MASK, index)
}

/// Derives a per-output sender ephemeral nonce bound to output `index`.
pub(crate) fn derive_sender_nonce(seed: &BuildSeed, index: u64) -> SecretKeyBytes {
    derive_scalar(seed, LABEL_SENDER_NONCE, index)
}

/// Derives a per-output AEAD nonce scalar bound to output `index`. Only `aead_nonce[..24]` is the
/// XChaCha20-Poly1305 nonce; the trailing 8 bytes are unused.
pub(crate) fn derive_aead_nonce(seed: &BuildSeed, index: u64) -> SecretKeyBytes {
    derive_scalar(seed, LABEL_AEAD_NONCE, index)
}

/// Derives a per-output ElGamal ephemeral nonce bound to output `index`.
pub(crate) fn derive_elgamal_nonce(seed: &BuildSeed, index: u64) -> SecretKeyBytes {
    derive_scalar(seed, LABEL_ELGAMAL_NONCE, index)
}

/// Derives the three viewable-balance ZK nonces `[x_v, x_m, x_r]` bound to output `index`.
pub(crate) fn derive_zk_nonces(seed: &BuildSeed, index: u64) -> [SecretKeyBytes; 3] {
    [
        derive_scalar(seed, LABEL_ZK_XV, index),
        derive_scalar(seed, LABEL_ZK_XM, index),
        derive_scalar(seed, LABEL_ZK_XR, index),
    ]
}

#[cfg(test)]
mod tests {
    use tari_crypto::ristretto::RistrettoSecretKey;

    use super::*;

    fn seed(byte: u8) -> BuildSeed {
        BuildSeed::from_array([byte; 32])
    }

    fn is_nonzero(s: &SecretKeyBytes) -> bool {
        s.as_bytes().iter().any(|&b| b != 0)
    }

    fn is_canonical(s: &SecretKeyBytes) -> bool {
        RistrettoSecretKey::from_canonical_bytes(s.as_bytes()).is_ok()
    }

    #[test]
    fn derivation_is_deterministic() {
        let s = seed(0x11);
        assert_eq!(derive_mask(&s, 0), derive_mask(&s, 0));
        assert_eq!(derive_zk_nonces(&s, 0), derive_zk_nonces(&s, 0));
    }

    #[test]
    fn distinct_seeds_yield_distinct_scalars() {
        assert_ne!(derive_mask(&seed(0x11), 0), derive_mask(&seed(0x22), 0));
    }

    #[test]
    fn index_binds_the_scalar() {
        let s = seed(0x11);
        // Same label, different output index ⇒ different scalar: cross-output reuse is structural.
        assert_ne!(derive_mask(&s, 0), derive_mask(&s, 1));
        let xm0 = derive_zk_nonces(&s, 0)[1].clone();
        let xm1 = derive_zk_nonces(&s, 1)[1].clone();
        assert_ne!(xm0, xm1);
    }

    #[test]
    fn labels_separate_scalars() {
        let s = seed(0x11);
        let scalars = [
            derive_mask(&s, 0),
            derive_sender_nonce(&s, 0),
            derive_aead_nonce(&s, 0),
            derive_elgamal_nonce(&s, 0),
            derive_zk_nonces(&s, 0)[0].clone(),
            derive_zk_nonces(&s, 0)[1].clone(),
            derive_zk_nonces(&s, 0)[2].clone(),
        ];
        for (i, a) in scalars.iter().enumerate() {
            assert!(is_nonzero(a) && is_canonical(a), "scalar {i} non-zero canonical");
            for (j, b) in scalars.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "labels {i} and {j} must derive distinct scalars");
                }
            }
        }
    }
}
