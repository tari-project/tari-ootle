//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! u64/u128-safe boundary numerics.
//!
//! The cardinal rule: a conceptual `u64` is a Rust `u64` at the boundary (never a string, never a
//! `[lo, hi]` pair), and a conceptual `u128` is the explicit [`U128`] `{ hi, lo }` record. The
//! native `u128` / [`Amount`](tari_template_lib_types::Amount) stays *internal* — it never crosses
//! the FFI boundary — which structurally removes the JS-`Number`-style 2^53 truncation class.

use serde::{Deserialize, Serialize};
use tari_template_lib_types::Amount;

use crate::types::error::OotleSdkError;

/// The uniform boundary representation of a 128-bit unsigned value: two `u64` words.
///
/// Hosts that lack a native 128-bit integer reassemble the value as `(hi as u128) << 64 | lo`. This
/// is the *only* shape a `u128` ever takes at the boundary; native `u128` / [`Amount`] stays
/// internal. Public transfers only need [`Amount`] (a `u64` µTari) today; `U128` is part of the
/// contract so the wire shape stays stable if a `u128` field is added later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct U128 {
    /// The high (most-significant) 64 bits.
    pub hi: u64,
    /// The low (least-significant) 64 bits.
    pub lo: u64,
}

impl U128 {
    /// Constructs a `U128` from its two `u64` words.
    pub const fn new(hi: u64, lo: u64) -> Self {
        Self { hi, lo }
    }

    /// Reassembles the native `u128`.
    pub const fn to_u128(self) -> u128 {
        ((self.hi as u128) << 64) | (self.lo as u128)
    }

    /// Splits a native `u128` into its two boundary words.
    pub const fn from_u128(value: u128) -> Self {
        Self {
            hi: (value >> 64) as u64,
            lo: value as u64,
        }
    }

    /// Splits an internal [`Amount`] into its boundary words.
    pub fn from_amount(amount: Amount) -> Self {
        Self::from_u128(amount.to_u128())
    }

    /// Reassembles an internal [`Amount`] from the boundary words.
    pub fn to_amount(self) -> Amount {
        Amount::new(self.to_u128())
    }
}

impl From<u128> for U128 {
    fn from(value: u128) -> Self {
        Self::from_u128(value)
    }
}

impl From<U128> for u128 {
    fn from(value: U128) -> Self {
        value.to_u128()
    }
}

/// A boundary token amount in **µTari** (micro-Tari): `1 TARI = 1_000_000 µTari`.
///
/// This is a plain `u64` at the boundary. It maps to the internal `u128`-backed [`Amount`] via
/// [`Amount::new`]; reading back uses [`Amount::to_u64_checked`], and a too-large internal value
/// becomes [`OotleSdkError::Validation`] rather than panicking or truncating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BoundaryAmount(pub u64);

impl BoundaryAmount {
    /// The number of µTari in one TARI.
    pub const MICRO_TARI_PER_TARI: u64 = 1_000_000;

    /// Constructs a boundary amount from a raw µTari `u64`.
    pub const fn new(micro_tari: u64) -> Self {
        Self(micro_tari)
    }

    /// Returns the raw µTari value.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Converts to the internal `u128`-backed [`Amount`].
    pub fn to_internal(self) -> Amount {
        Amount::new(u128::from(self.0))
    }

    /// Builds a boundary amount from an internal [`Amount`], failing (rather than truncating) if the
    /// internal value does not fit a `u64`.
    pub fn from_internal(amount: Amount) -> Result<Self, OotleSdkError> {
        amount.to_u64_checked().map(Self).ok_or_else(|| {
            OotleSdkError::Validation(format!(
                "amount {} exceeds the u64 µTari boundary range",
                amount.to_u128()
            ))
        })
    }
}

impl From<u64> for BoundaryAmount {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<BoundaryAmount> for u64 {
    fn from(value: BoundaryAmount) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u128_round_trips_small() {
        let v = 42u128;
        assert_eq!(U128::from(v).to_u128(), v);
        assert_eq!(u128::from(U128::from(v)), v);
    }

    #[test]
    fn u128_round_trips_above_2_pow_53() {
        // 2^53 + 1 — the value JS `Number` cannot represent exactly. Proves the truncation class is
        // structurally gone.
        let v: u128 = (1u128 << 53) + 1;
        let b = U128::from(v);
        assert_eq!(b.hi, 0);
        assert_eq!(b.lo, (1u64 << 53) + 1);
        assert_eq!(b.to_u128(), v);
    }

    #[test]
    fn u128_round_trips_full_width() {
        // A value > u64::MAX exercising the high word, and the all-ones edge.
        for v in [(1u128 << 64) + 7, u128::MAX, u128::from(u64::MAX)] {
            assert_eq!(U128::from(v).to_u128(), v, "round-trip failed for {v}");
        }
        let max = U128::from(u128::MAX);
        assert_eq!(max.hi, u64::MAX);
        assert_eq!(max.lo, u64::MAX);
    }

    #[test]
    fn u128_amount_round_trip() {
        let amount = Amount::new((1u128 << 70) + 123);
        let b = U128::from_amount(amount);
        assert_eq!(b.to_amount(), amount);
    }

    #[test]
    fn boundary_amount_round_trips_via_internal() {
        // Well above 2^53 µTari to prove no truncation on the Amount path.
        let micro: u64 = (1u64 << 53) + 99;
        let amount = BoundaryAmount::new(micro);
        let internal = amount.to_internal();
        assert_eq!(internal.to_u128(), u128::from(micro));
        assert_eq!(BoundaryAmount::from_internal(internal).unwrap(), amount);
    }

    #[test]
    fn boundary_amount_u64_max_round_trips() {
        let amount = BoundaryAmount::new(u64::MAX);
        assert_eq!(BoundaryAmount::from_internal(amount.to_internal()).unwrap(), amount);
    }

    #[test]
    fn boundary_amount_rejects_overflow_internal() {
        // An internal Amount one past u64::MAX must validate-fail, never truncate or panic.
        let too_big = Amount::new(u128::from(u64::MAX) + 1);
        let err = BoundaryAmount::from_internal(too_big).unwrap_err();
        assert_eq!(err.code(), "VALIDATION");
    }

    #[test]
    fn boundary_amount_serde_is_plain_number() {
        let json = serde_json::to_string(&BoundaryAmount::new(1_500_000)).unwrap();
        assert_eq!(json, "1500000");
    }
}
