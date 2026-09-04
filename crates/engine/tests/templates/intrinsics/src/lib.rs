//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Exercises `tari_template_lib::intrinsics` from inside a template, so the tests can check each
//! primitive against a value computed natively on the host.
//!
//! The functions returning composites (`homomorphism`, `pedersen_commit`) are the point: an
//! intrinsic that is individually correct can still be wired up wrongly, and an algebraic identity
//! that only holds when several of them agree catches that.

use tari_template_lib::prelude::*;

#[template]
mod intrinsics_test {
    use super::*;

    pub struct IntrinsicsTest {}

    impl IntrinsicsTest {
        // --- group ----------------------------------------------------------------------
        pub fn ristretto_add(a: RistrettoPublicKeyBytes, b: RistrettoPublicKeyBytes) -> RistrettoPublicKeyBytes {
            intrinsics::ristretto_add(&a, &b)
        }

        pub fn ristretto_sub(a: RistrettoPublicKeyBytes, b: RistrettoPublicKeyBytes) -> RistrettoPublicKeyBytes {
            intrinsics::ristretto_sub(&a, &b)
        }

        pub fn ristretto_negate(a: RistrettoPublicKeyBytes) -> RistrettoPublicKeyBytes {
            intrinsics::ristretto_negate(&a)
        }

        pub fn ristretto_mul(p: RistrettoPublicKeyBytes, s: Scalar32Bytes) -> RistrettoPublicKeyBytes {
            intrinsics::ristretto_mul(&p, &s)
        }

        pub fn ristretto_mul_base(s: Scalar32Bytes) -> RistrettoPublicKeyBytes {
            intrinsics::ristretto_mul_base(&s)
        }

        pub fn ristretto_msm(points: Vec<RistrettoPublicKeyBytes>, scalars: Vec<Scalar32Bytes>) -> RistrettoPublicKeyBytes {
            intrinsics::ristretto_msm(&points, &scalars)
        }

        pub fn ristretto_is_identity(a: RistrettoPublicKeyBytes) -> bool {
            intrinsics::ristretto_is_identity(&a)
        }

        pub fn ristretto_is_canonical(a: RistrettoPublicKeyBytes) -> bool {
            intrinsics::ristretto_is_canonical(&a)
        }

        // --- scalar field ---------------------------------------------------------------
        pub fn scalar_add(a: Scalar32Bytes, b: Scalar32Bytes) -> Scalar32Bytes {
            intrinsics::scalar_add(&a, &b)
        }

        pub fn scalar_sub(a: Scalar32Bytes, b: Scalar32Bytes) -> Scalar32Bytes {
            intrinsics::scalar_sub(&a, &b)
        }

        pub fn scalar_mul(a: Scalar32Bytes, b: Scalar32Bytes) -> Scalar32Bytes {
            intrinsics::scalar_mul(&a, &b)
        }

        pub fn scalar_negate(a: Scalar32Bytes) -> Scalar32Bytes {
            intrinsics::scalar_negate(&a)
        }

        pub fn scalar_invert(a: Scalar32Bytes) -> Option<Scalar32Bytes> {
            intrinsics::scalar_invert(&a)
        }

        pub fn scalar_is_canonical(a: Scalar32Bytes) -> bool {
            intrinsics::scalar_is_canonical(&a)
        }

        // --- hashing ---------------------------------------------------------------------
        pub fn hash_blake2b(data: Vec<u8>) -> Hash32 {
            intrinsics::hash_blake2b(&data)
        }

        pub fn hash_sha256(data: Vec<u8>) -> Hash32 {
            intrinsics::hash_sha256(&data)
        }

        pub fn hash_keccak256(data: Vec<u8>) -> Hash32 {
            intrinsics::hash_keccak256(&data)
        }

        pub fn hash_sha512(data: Vec<u8>) -> Hash64 {
            intrinsics::hash_sha512(&data)
        }

        /// Hashing two parts must equal hashing their concatenation — the property that lets a
        /// Merkle walk pay one engine crossing per level instead of one per byte range.
        pub fn hash_parts_matches_concat(a: Vec<u8>, b: Vec<u8>) -> bool {
            let mut joined = a.clone();
            joined.extend_from_slice(&b);
            intrinsics::hash_blake2b_parts(&[&a, &b]) == intrinsics::hash_blake2b(&joined)
        }

        // --- composites ------------------------------------------------------------------
        /// `(a + b) * G == a*G + b*G`. Holds only if scalar addition, fixed-base multiplication and
        /// point addition all agree, so it catches a primitive that is wired to the wrong operation.
        pub fn homomorphism(a: Scalar32Bytes, b: Scalar32Bytes) -> bool {
            let lhs = intrinsics::ristretto_mul_base(&intrinsics::scalar_add(&a, &b));
            let rhs = intrinsics::ristretto_add(
                &intrinsics::ristretto_mul_base(&a),
                &intrinsics::ristretto_mul_base(&b),
            );
            lhs == rhs
        }

        /// A Pedersen commitment `v*H + r*G`, built from the primitives a template actually has.
        /// The shape any confidential scheme written in a template would start from.
        pub fn pedersen_commit(
            value: Scalar32Bytes,
            mask: Scalar32Bytes,
            h: RistrettoPublicKeyBytes,
        ) -> RistrettoPublicKeyBytes {
            intrinsics::ristretto_add(
                &intrinsics::ristretto_mul(&h, &value),
                &intrinsics::ristretto_mul_base(&mask),
            )
        }

        /// Commitments are additively homomorphic: `commit(v1,r1) + commit(v2,r2) == commit(v1+v2, r1+r2)`.
        pub fn commitments_are_homomorphic(
            v1: Scalar32Bytes,
            r1: Scalar32Bytes,
            v2: Scalar32Bytes,
            r2: Scalar32Bytes,
            h: RistrettoPublicKeyBytes,
        ) -> bool {
            let sum = intrinsics::ristretto_add(
                &Self::pedersen_commit(v1, r1, h),
                &Self::pedersen_commit(v2, r2, h),
            );
            let combined = Self::pedersen_commit(
                intrinsics::scalar_add(&v1, &v2),
                intrinsics::scalar_add(&r1, &r2),
                h,
            );
            sum == combined
        }

        /// A multi-scalar multiplication must equal the same terms multiplied and summed one by one.
        pub fn msm_matches_loop(points: Vec<RistrettoPublicKeyBytes>, scalars: Vec<Scalar32Bytes>) -> bool {
            let mut acc: Option<RistrettoPublicKeyBytes> = None;
            for (p, s) in points.iter().zip(scalars.iter()) {
                let term = intrinsics::ristretto_mul(p, s);
                acc = Some(match acc {
                    Some(a) => intrinsics::ristretto_add(&a, &term),
                    None => term,
                });
            }
            intrinsics::ristretto_msm(&points, &scalars) == acc.expect("at least one term")
        }
    }
}
