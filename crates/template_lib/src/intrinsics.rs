//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Native primitives a template can call instead of implementing them in WASM.
//!
//! Each is a pure function — values in, a value out. None reads or writes substate, allocates an
//! address, emits an event, or observes anything about its caller. That is what lets the engine
//! price one from its declared arguments before running it, and what makes a template's use of one
//! reproducible on every validator.
//!
//! # Cost
//!
//! An intrinsic is charged in native metering points against the same budget as WASM execution, so
//! calling one is not free — it is far cheaper than the equivalent compiled to WASM, not free. The
//! charge is taken before the work runs, so a transaction that cannot afford one traps without the
//! validator having done it.
//!
//! # Failure
//!
//! Malformed input panics, aborting the transaction. That is the right outcome for a caller bug and
//! it keeps the common path clear of `Result` threading. A template handling untrusted bytes checks
//! them once at its boundary with [`ristretto_is_canonical`] or [`scalar_is_canonical`], then
//! operates freely.
//!
//! An [`Option`] or `bool` return means the absence of an answer is itself the result — a zero
//! scalar has no inverse, a signature is or is not valid. It never signals a malformed argument.
//!
//! # Extending the set
//!
//! Intrinsics are addressed by a permanent numeric [`IntrinsicId`]. Adding one changes no wire type
//! and leaves every published template working, but a template that calls a new intrinsic needs
//! validators and indexers that know it — a coordinated upgrade. One that does not know an id
//! rejects the call naming that id.

use tari_template_abi::{EngineOp, call_engine, rust::prelude::*};
use tari_template_lib_types::{
    Hash32,
    Hash64,
    bytes::{Bytes, BytesRef},
    crypto::{PublicKey, RistrettoPublicKeyBytes, Scalar32Bytes, SignaturePayload},
    engine_args::{IntrinsicId, IntrinsicInvokeArg, SignatureVerifyArgRef},
};

use crate::args::InvokeResult;

/// Calls an intrinsic and decodes its result.
///
/// A decode failure is not a template error: each intrinsic has one defined return type, so a
/// mismatch means this template library and the validator disagree about that definition.
fn invoke<T>(intrinsic: IntrinsicId, args: Vec<Bytes>) -> T
where T: for<'a> tari_bor::Decode<'a, ()> {
    let result: InvokeResult = call_engine(EngineOp::IntrinsicInvoke, &IntrinsicInvokeArg { intrinsic, args });
    result
        .decode()
        .expect("engine returned a value of an unexpected type for this intrinsic")
}

// -------------------------------- Ristretto group -------------------------------- //

/// `a + b` on the Ristretto group.
pub fn ristretto_add(a: &RistrettoPublicKeyBytes, b: &RistrettoPublicKeyBytes) -> RistrettoPublicKeyBytes {
    invoke(IntrinsicId::RISTRETTO_ADD, invoke_args![a, b])
}

/// `a - b` on the Ristretto group.
pub fn ristretto_sub(a: &RistrettoPublicKeyBytes, b: &RistrettoPublicKeyBytes) -> RistrettoPublicKeyBytes {
    invoke(IntrinsicId::RISTRETTO_SUB, invoke_args![a, b])
}

/// `-a` on the Ristretto group.
pub fn ristretto_negate(a: &RistrettoPublicKeyBytes) -> RistrettoPublicKeyBytes {
    invoke(IntrinsicId::RISTRETTO_NEGATE, invoke_args![a])
}

/// `point * scalar` — variable-base scalar multiplication.
pub fn ristretto_mul(point: &RistrettoPublicKeyBytes, scalar: &Scalar32Bytes) -> RistrettoPublicKeyBytes {
    invoke(IntrinsicId::RISTRETTO_MUL, invoke_args![point, scalar])
}

/// `G * scalar`, where `G` is the Ristretto basepoint — the cheaper fixed-base case.
pub fn ristretto_mul_base(scalar: &Scalar32Bytes) -> RistrettoPublicKeyBytes {
    invoke(IntrinsicId::RISTRETTO_MUL_BASE, invoke_args![scalar])
}

/// `sum(points[i] * scalars[i])` as a single multi-scalar multiplication.
///
/// Cheaper than the same number of [`ristretto_mul`] calls, because the engine batches the terms,
/// so prefer this to a loop whenever there is more than one.
///
/// # Panics
///
/// If the slices differ in length.
pub fn ristretto_msm(points: &[RistrettoPublicKeyBytes], scalars: &[Scalar32Bytes]) -> RistrettoPublicKeyBytes {
    assert_eq!(
        points.len(),
        scalars.len(),
        "ristretto_msm needs one scalar per point, got {} points and {} scalars",
        points.len(),
        scalars.len(),
    );
    invoke(IntrinsicId::RISTRETTO_MSM, invoke_args![points, scalars])
}

/// Whether `a` is the group identity.
///
/// A routine check on a counterparty-supplied key: the identity collapses any product it appears
/// in, so accepting one can silently void a protocol's binding.
pub fn ristretto_is_identity(a: &RistrettoPublicKeyBytes) -> bool {
    invoke(IntrinsicId::RISTRETTO_IS_IDENTITY, invoke_args![a])
}

/// Whether `bytes` is the canonical encoding of a Ristretto point.
///
/// The boundary check for untrusted input: every other Ristretto intrinsic panics on bytes that are
/// not. Costs about one group operation, since it is the decompression the others already perform.
pub fn ristretto_is_canonical(bytes: &RistrettoPublicKeyBytes) -> bool {
    invoke(IntrinsicId::RISTRETTO_IS_CANONICAL, invoke_args![bytes])
}

// -------------------------------- Scalar field -------------------------------- //

/// `a + b` in the scalar field.
pub fn scalar_add(a: &Scalar32Bytes, b: &Scalar32Bytes) -> Scalar32Bytes {
    invoke(IntrinsicId::SCALAR_ADD, invoke_args![a, b])
}

/// `a - b` in the scalar field.
pub fn scalar_sub(a: &Scalar32Bytes, b: &Scalar32Bytes) -> Scalar32Bytes {
    invoke(IntrinsicId::SCALAR_SUB, invoke_args![a, b])
}

/// `a * b` in the scalar field.
pub fn scalar_mul(a: &Scalar32Bytes, b: &Scalar32Bytes) -> Scalar32Bytes {
    invoke(IntrinsicId::SCALAR_MUL, invoke_args![a, b])
}

/// `-a` in the scalar field.
pub fn scalar_negate(a: &Scalar32Bytes) -> Scalar32Bytes {
    invoke(IntrinsicId::SCALAR_NEGATE, invoke_args![a])
}

/// The multiplicative inverse of `a`, or `None` when `a` is zero.
///
/// Zero is a well-formed scalar that simply has no inverse, so the missing answer is the result
/// rather than an input error.
pub fn scalar_invert(a: &Scalar32Bytes) -> Option<Scalar32Bytes> {
    invoke(IntrinsicId::SCALAR_INVERT, invoke_args![a])
}

/// Reduces 64 uniformly distributed bytes to a scalar.
///
/// The way to turn a hash into a scalar. Reducing 32 bytes modulo the group order is measurably
/// biased; wide reduction from 64 is not.
pub fn scalar_from_uniform_bytes(bytes: &[u8; 64]) -> Scalar32Bytes {
    invoke(IntrinsicId::SCALAR_FROM_UNIFORM_BYTES, invoke_args![BytesRef::new(
        bytes
    )])
}

/// Whether `bytes` is a canonically reduced scalar — strictly less than the group order.
///
/// Worth checking on anything a counterparty supplied. [`Scalar32Bytes`] is a raw byte wrapper, and
/// a non-canonical encoding is a malleability vector: two distinct byte strings reducing to the same
/// scalar let a signature or commitment be replayed under a second encoding.
pub fn scalar_is_canonical(bytes: &Scalar32Bytes) -> bool {
    invoke(IntrinsicId::SCALAR_IS_CANONICAL, invoke_args![bytes])
}

// -------------------------------- Hashing -------------------------------- //

macro_rules! hash_fns {
    ($id:expr, $one:ident, $parts:ident, $out:ty, $alg:literal) => {
        #[doc = concat!("The ", $alg, " digest of `data`.")]
        pub fn $one(data: &[u8]) -> $out {
            $parts(&[data])
        }

        #[doc = concat!("The ", $alg, " digest of every part concatenated, in order.")]
        /// One engine call rather than one per part, which is what makes walking a Merkle path
        /// affordable: the parts are concatenated engine-side and hashed once.
        pub fn $parts(parts: &[&[u8]]) -> $out {
            let args = parts.iter().copied().map(BytesRef::new).collect::<Vec<_>>();
            invoke($id, invoke_args![args])
        }
    };
}

hash_fns!(
    IntrinsicId::HASH_BLAKE2B,
    hash_blake2b,
    hash_blake2b_parts,
    Hash32,
    "Blake2b-256"
);
hash_fns!(
    IntrinsicId::HASH_SHA256,
    hash_sha256,
    hash_sha256_parts,
    Hash32,
    "SHA-256"
);
hash_fns!(
    IntrinsicId::HASH_KECCAK256,
    hash_keccak256,
    hash_keccak256_parts,
    Hash32,
    "Keccak-256"
);
hash_fns!(
    IntrinsicId::HASH_SHA512,
    hash_sha512,
    hash_sha512_parts,
    Hash64,
    "SHA-512"
);

// -------------------------------- Signatures -------------------------------- //

/// Whether `payload` is a valid Ristretto Schnorr signature by `public_key` over `message`, within
/// `domain`.
///
/// The domain is bound into what is signed, so a signature made for one domain never verifies under
/// another — which is what stops one being replayed into a context it was not made for.
///
/// Templates normally reach this through [`Signature::verify`], which takes the domain from the
/// signature's type parameter; call this directly when the domain is only known at runtime.
///
/// [`Signature::verify`]: crate::models::Verifiable::verify
pub fn schnorr_verify_with_domain(
    public_key: &PublicKey,
    domain: &[u8],
    message: &[u8],
    payload: &SignaturePayload,
) -> bool {
    invoke(IntrinsicId::SCHNORR_VERIFY, invoke_args![SignatureVerifyArgRef {
        public_key,
        domain,
        message,
        payload,
    }])
}
