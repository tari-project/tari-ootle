//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};
use tari_template_abi::rust::{fmt, prelude::*};

use crate::{
    bytes::Bytes,
    crypto::{PublicKey, SignaturePayload},
};

// -------------------------------- Signature -------------------------------- //
#[derive(Debug, Clone, Encode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SignatureVerifyArgRef<'a> {
    #[n(0)]
    pub public_key: &'a PublicKey,
    #[n(1)]
    #[cfg_attr(feature = "serde", serde(serialize_with = "crate::bytes::serialize"))]
    #[cbor(with = "minicbor::bytes")]
    pub domain: &'a [u8],
    #[n(2)]
    #[cfg_attr(feature = "serde", serde(serialize_with = "crate::bytes::serialize"))]
    #[cbor(with = "minicbor::bytes")]
    pub message: &'a [u8],
    #[n(3)]
    pub payload: &'a SignaturePayload,
}

#[derive(Debug, Clone, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct SignatureVerifyArg {
    #[n(0)]
    pub public_key: PublicKey,
    #[n(1)]
    pub domain: Bytes,
    #[n(2)]
    pub message: Bytes,
    #[n(3)]
    pub payload: SignaturePayload,
}

// -------------------------------- Intrinsics -------------------------------- //

/// Wire identifier for a native intrinsic.
///
/// A plain integer rather than an enum, because the set grows over time and the two failure modes
/// are not equivalent. A validator that does not know an id answers `IntrinsicNotSupported`, naming
/// the id — the correct diagnosis when a template built against a newer `tari_template_lib` runs on
/// a validator that has not been upgraded yet. An enum would instead fail in the CBOR decoder as an
/// unknown variant, which reads like a malformed transaction.
///
/// Ids are permanent: never renumbered, and never reused once retired. Adding one is a coordinated
/// validator and indexer upgrade, but changes no wire type and leaves every already-published
/// template working unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode, CborLen)]
#[cbor(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
pub struct IntrinsicId(#[n(0)] pub u32);

impl IntrinsicId {
    /// Every id this library knows. The engine asserts each one is both priced and dispatched, so a
    /// new id added here without both arms fails the build rather than reaching a validator.
    pub const ALL: &'static [Self] = &[
        Self::RISTRETTO_ADD,
        Self::RISTRETTO_SUB,
        Self::RISTRETTO_NEGATE,
        Self::RISTRETTO_MUL,
        Self::RISTRETTO_MUL_BASE,
        Self::RISTRETTO_MSM,
        Self::RISTRETTO_IS_IDENTITY,
        Self::RISTRETTO_IS_CANONICAL,
        Self::SCALAR_ADD,
        Self::SCALAR_SUB,
        Self::SCALAR_MUL,
        Self::SCALAR_NEGATE,
        Self::SCALAR_INVERT,
        Self::SCALAR_FROM_UNIFORM_BYTES,
        Self::SCALAR_IS_CANONICAL,
        Self::HASH_BLAKE2B,
        Self::HASH_SHA256,
        Self::HASH_SHA512,
        Self::HASH_KECCAK256,
        Self::SCHNORR_VERIFY,
    ];
    /// The count is asserted so `ALL` cannot silently fall behind the constants above: adding one
    /// without listing it fails to compile here rather than slipping past the engine's
    /// priced-and-dispatched guard, which only iterates what `ALL` contains.
    pub const ALL_LEN: usize = 20;
    // Hashing. 0x0020-0x002F. Each hashes the concatenation of every argument, in order.
    pub const HASH_BLAKE2B: Self = Self(0x0020);
    pub const HASH_KECCAK256: Self = Self(0x0023);
    pub const HASH_SHA256: Self = Self(0x0021);
    pub const HASH_SHA512: Self = Self(0x0022);
    // Ristretto group operations. 0x0001-0x000F.
    pub const RISTRETTO_ADD: Self = Self(0x0001);
    pub const RISTRETTO_IS_CANONICAL: Self = Self(0x0008);
    pub const RISTRETTO_IS_IDENTITY: Self = Self(0x0007);
    pub const RISTRETTO_MSM: Self = Self(0x0006);
    pub const RISTRETTO_MUL: Self = Self(0x0004);
    pub const RISTRETTO_MUL_BASE: Self = Self(0x0005);
    pub const RISTRETTO_NEGATE: Self = Self(0x0003);
    pub const RISTRETTO_SUB: Self = Self(0x0002);
    // Scalar field arithmetic. 0x0010-0x001F.
    pub const SCALAR_ADD: Self = Self(0x0010);
    pub const SCALAR_FROM_UNIFORM_BYTES: Self = Self(0x0015);
    pub const SCALAR_INVERT: Self = Self(0x0014);
    pub const SCALAR_IS_CANONICAL: Self = Self(0x0016);
    pub const SCALAR_MUL: Self = Self(0x0012);
    pub const SCALAR_NEGATE: Self = Self(0x0013);
    pub const SCALAR_SUB: Self = Self(0x0011);
    // Signatures. 0x0030-0x003F.
    pub const SCHNORR_VERIFY: Self = Self(0x0030);

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

const _: () = assert!(
    IntrinsicId::ALL.len() == IntrinsicId::ALL_LEN,
    "IntrinsicId::ALL is missing an id (or ALL_LEN is stale)"
);

impl fmt::Display for IntrinsicId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "intrinsic#{:#06x}", self.0)
    }
}

#[derive(Clone, Debug, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IntrinsicInvokeArg {
    #[n(0)]
    pub intrinsic: IntrinsicId,
    #[n(1)]
    pub args: Vec<Bytes>,
}
