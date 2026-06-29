//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Native digest for the [`BuiltinPredicate::HashLock`](tari_template_lib::types::stealth::BuiltinPredicate::HashLock)
//! spend condition (TIP-0006).
//!
//! The preimage is hashed with no domain separation so a hashlock can interoperate with an external chain's HTLC (e.g.
//! Bitcoin's `SHA256`). Hashing is native-only; a template never computes it.

use blake2::Blake2b;
use digest::{Digest, consts::U32, generic_array::GenericArray};
use sha2::Sha256;
use tari_template_lib::types::{Hash32, stealth::HashAlg};

/// Computes the [`HashAlg`] digest of `preimage`, for comparison against a committed `HashLock` digest.
pub fn hashlock_digest(alg: HashAlg, preimage: &[u8]) -> Hash32 {
    let mut out = [0u8; 32];
    match alg {
        HashAlg::Blake2b256 => {
            Blake2b::<U32>::new()
                .chain_update(preimage)
                .finalize_into(GenericArray::from_mut_slice(out.as_mut()));
        },
        HashAlg::Sha256 => {
            Sha256::new()
                .chain_update(preimage)
                .finalize_into(GenericArray::from_mut_slice(out.as_mut()));
        },
    }
    Hash32::from_array(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algorithms_are_distinct_and_deterministic() {
        let preimage = b"open sesame";
        let blake = hashlock_digest(HashAlg::Blake2b256, preimage);
        let sha = hashlock_digest(HashAlg::Sha256, preimage);
        assert_ne!(blake, sha);
        assert_eq!(blake, hashlock_digest(HashAlg::Blake2b256, preimage));
        assert_ne!(blake, hashlock_digest(HashAlg::Blake2b256, b"different"));
    }

    #[test]
    fn sha256_matches_a_known_vector() {
        // SHA-256("abc"), the canonical NIST test vector — a hashlock must agree byte-for-byte with external chains.
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let digest = hashlock_digest(HashAlg::Sha256, b"abc");
        assert_eq!(hex::encode(digest.into_array()), expected);
    }
}
