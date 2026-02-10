//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use serde::ser::Error;
use tari_template_abi::rust::{cmp, fmt, iter::Sum, num, ops, prelude::*, str::FromStr, write};

use crate::{impl_from, impl_try_from, partial_eq_impl, partial_ord_impl};

/// A 128-bit signed amount.
///
/// This is a general purpose signed integer, but is primarily used to represent the smallest unit of value in
/// resources/vaults etc.
///
/// This allows Tari to support a massive number tokens within resources.
/// e.g. 2 ETH = 2 x 10^18 Gwei.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Amount(#[cfg_attr(feature = "ts", ts(type = "string | number"))] pub(super) u128);

impl Amount {
    pub const BITS: usize = u128::BITS as usize;
    pub const BYTE_SIZE: usize = Self::BITS / 8;
    /// The maximum value that an `Amount` can hold.
    pub const MAX: Self = Self(u128::MAX);
    /// The minimum value that an `Amount` can hold.
    pub const MIN: Self = Self(u128::MIN);
    /// The number of u64 digits used to represent a Amount i.e. 2
    pub const NUM_U64_DIGITS: usize = Self::BITS / u64::BITS as usize;
    /// The value of one, represented as an `Amount`.
    pub const ONE: Self = Self(1);
    /// The value of one hundred, represented as an `Amount`.
    pub const ONE_HUNDRED: Self = Self::from_le_digits([100, 0]);
    /// The value of one thousand, represented as an `Amount`.
    pub const ONE_THOUSAND: Self = Self::from_le_digits([1000, 0]);
    /// The value of ten, represented as an `Amount`.
    pub const TEN: Self = Self::from_le_digits([10, 0]);
    /// The value of zero, represented as an `Amount`.
    pub const ZERO: Self = Self(0);

    /// Creates a new `Amount` from an integer value.
    pub const fn new(amount: u128) -> Self {
        Self(amount)
    }

    /// Creates a new `Amount` from an integer value.
    pub fn from_integer<T: Into<u128>>(amount: T) -> Self {
        Self::new(amount.into())
    }

    pub const fn from_usize(value: usize) -> Self {
        Self(value as u128)
    }

    /// A value of zero.
    pub const fn zero() -> Self {
        Self::ZERO
    }

    /// Returns true if the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Returns true if the amount is positive (greater than zero).
    pub const fn is_positive(&self) -> bool {
        self.0 > 0
    }

    /// Returns true if the amount is non-negative (greater than or equal to zero).
    pub const fn is_non_negative(&self) -> bool {
        true
    }

    /// Returns true if the amount is negative (less than zero).
    pub const fn is_negative(&self) -> bool {
        false
    }

    pub const fn from_u64(value: u64) -> Self {
        // Until const u64 to u128 conversion is stable we'll do this cast
        Self(value as u128)
    }

    /// Returns the inner value of this amount as an `u128`.
    const fn inner_value(&self) -> &u128 {
        &self.0
    }

    const fn into_inner_value(self) -> u128 {
        self.0
    }

    pub const fn to_u128(&self) -> u128 {
        self.0
    }

    /// Returns the value of this amount + other. Returns `None` if the result underflows or overflows.
    pub const fn checked_add(&self, other: Self) -> Option<Self> {
        // match is because const map operations are not yet stable
        match self.inner_value().checked_add(other.into_inner_value()) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the sum of two amounts, saturating at `i64::MAX` if the result exceeds it.
    pub const fn saturating_add(&self, other: Self) -> Self {
        Self(self.into_inner_value().saturating_add(other.into_inner_value()))
    }

    /// Returns the difference of two amounts, saturating at `0` if the result is negative.
    pub const fn checked_sub(&self, other: Self) -> Option<Self> {
        match self.into_inner_value().checked_sub(other.into_inner_value()) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub fn sum_from_positive<A: Into<Self>, I: Iterator<Item = A>>(iter: I) -> Option<Self> {
        let mut sum = Self::zero();
        for amount in iter {
            sum = sum.checked_add(amount.into())?;
        }
        Some(sum)
    }

    /// Returns the difference of two amounts, saturating at `Amount::MIN` if the result underflows.
    /// If negative results are not desired, use `saturating_sub_positive`.
    pub const fn saturating_sub(&self, other: Self) -> Self {
        Self(self.into_inner_value().saturating_sub(other.into_inner_value()))
    }

    /// Returns the difference of two amounts, returning `None` if the result is negative or if either amount is
    /// negative.
    pub fn checked_sub_positive(&self, other: Self) -> Option<Self> {
        if self.is_negative() || other.is_negative() {
            return None;
        }
        if *self < other {
            return None;
        }

        self.checked_sub(other)
    }

    /// Returns the product of two amounts, returning `None` if the result overflows.
    pub const fn checked_mul(&self, other: Self) -> Option<Self> {
        match self.into_inner_value().checked_mul(other.into_inner_value()) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the product of two amounts, saturating at `i64::MAX` if the result exceeds it.
    pub const fn saturating_mul(&self, other: Self) -> Self {
        Self(self.into_inner_value().saturating_mul(other.into_inner_value()))
    }

    /// Returns the quotient of two amounts, returning `None` if the divisor is zero or if the result overflows.
    pub const fn checked_div(&self, other: Self) -> Option<Self> {
        match self.into_inner_value().checked_div(other.into_inner_value()) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the quotient of two amounts, returning `None` if the divisor is zero or if the result overflows.
    pub const fn checked_div_ceil(&self, other: Self) -> Option<Self> {
        if other.is_zero() {
            return None;
        }
        let n = self.into_inner_value();
        let d = other.into_inner_value();

        let div = match n.checked_div(d) {
            Some(value) => value,
            None => return None,
        };
        let rem = match n.checked_rem(d) {
            Some(value) => value,
            None => return None,
        };

        // If the remainder is zero or the result is negative, we round down
        if rem == 0 {
            Some(Self(div))
        } else {
            // Otherwise, we round up
            Some(Self(div + 1))
        }
    }

    /// Returns the quotient of two amounts, returning `None` if the divisor is zero or if the result overflows.
    ///
    /// # Panics
    /// If the divisor is zero, this function will panic.
    pub const fn div_ceil(&self, other: Self) -> Self {
        self.checked_div_ceil(other).expect("division by zero")
    }

    /// Returns the quotient of two amounts, saturating at `i64::MAX` if the result exceeds it.
    pub const fn saturating_div(&self, other: &Self) -> Self {
        Self(self.into_inner_value().saturating_div(other.into_inner_value()))
    }

    /// Returns the value as an u64 if possible, otherwise returns None.
    /// Since the internal representation is i64, this will return None if the value is negative.
    pub fn to_u64_checked(&self) -> Option<u64> {
        self.into_inner_value().try_into().ok()
    }

    /// Returns the value as an BYTE_SIZE byte array in canonical order (little-endian).
    pub fn to_canonical_bytes(&self) -> [u8; Self::BYTE_SIZE] {
        self.to_le_bytes()
    }

    pub fn to_le_bytes(&self) -> [u8; Self::BYTE_SIZE] {
        self.0.to_le_bytes()
    }

    pub fn from_le_bytes(bytes: [u8; Self::BYTE_SIZE]) -> Self {
        Self(u128::from_le_bytes(bytes))
    }

    pub const fn from_le_digits(digits: [u64; Self::NUM_U64_DIGITS]) -> Self {
        let n = digits[0] as u128 | ((digits[1] as u128) << 64);
        Self(n)
    }

    /// Creates an integer value from a slice of bytes in little endian. The value is wrapped in an [`Option`](https://doc.rust-lang.org/core/option/enum.Option.html) as the bytes may represent an integer too large to be represented by the type.
    ///
    /// If the length of the slice is shorter than `Self::BYTES`, the slice is padded with zeros or ones at the end so
    /// that it's length equals `Self::BYTES`.
    ///
    /// If the length of the slice is longer than `Self::BYTES`, `None` will be returned, unless the bytes have
    /// trailing zeros that can be removed until the length of the slice equals `Self::BYTES`.
    pub fn from_le_slice(bytes: &[u8]) -> Option<Self> {
        let len = bytes.len();

        if len == 0 {
            return Some(Self::zero());
        }

        if len > Self::BYTE_SIZE {
            // Check for trailing zeros that can be removed
            let mut trimmed_len = len;
            while trimmed_len > Self::BYTE_SIZE && bytes[trimmed_len - 1] == 0 {
                trimmed_len -= 1;
            }
            if trimmed_len != Self::BYTE_SIZE {
                return None;
            }
            let mut trimmed_bytes = [0u8; Self::BYTE_SIZE];
            trimmed_bytes.copy_from_slice(&bytes[..Self::BYTE_SIZE]);
            return Some(Self(u128::from_le_bytes(trimmed_bytes)));
        }

        let mut padded_bytes = [0u8; Self::BYTE_SIZE];
        padded_bytes[..len].copy_from_slice(bytes);
        Some(Self(u128::from_le_bytes(padded_bytes)))
    }

    pub fn to_le_digits(&self) -> [u64; Self::NUM_U64_DIGITS] {
        [
            (self.inner_value() & u128::from(u64::MAX)) as u64,
            ((self.inner_value() >> 64) & u128::from(u64::MAX)) as u64,
        ]
    }

    #[cfg(feature = "extra-arith")]
    pub fn checked_sqrt(&self) -> Option<Self> {
        use num_integer::Roots;
        if self.is_negative() {
            return None;
        }
        if self.is_zero() {
            return Some(Self::zero());
        }
        let inner = self.into_inner_value();
        let sqrt_inner = inner.sqrt();
        Some(Self(sqrt_inner))
    }

    /// If the amount is negative (< 0), returns `None`, otherwise returns `Some(self)`.
    pub fn non_negative_checked(self) -> Option<Self> {
        if self.is_negative() { None } else { Some(self) }
    }

    /// If the amount is positive (> 0), returns `None`, otherwise returns `Some(self)`.
    pub fn negative_checked(self) -> Option<Self> {
        if self.is_positive() { None } else { Some(self) }
    }

    /// Returns the amount raised to the power of `exp`.
    pub const fn pow(&self, exp: u32) -> Self {
        Self(self.into_inner_value().pow(exp))
    }

    /// Returns the amount raised to the power of `exp`, returning `None` if the result overflows.
    pub const fn checked_pow(&self, exp: u32) -> Option<Self> {
        match self.into_inner_value().checked_pow(exp) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Parses a string as an `Amount` in the specified radix.
    ///
    /// This function works in constant context, allowing it to be used to define constants.
    pub const fn try_from_str_radix(s: &str, radix: u32) -> Option<Self> {
        match u128::from_str_radix(s, radix) {
            Ok(value) => Some(Self(value)),
            Err(_) => None,
        }
    }

    /// Parses a string as an `Amount` in the specified radix.
    ///
    /// # Panics
    /// Panics if string parsing fails
    pub const fn from_str_radix(s: &str, radix: u32) -> Self {
        match Self::try_from_str_radix(s, radix) {
            Some(value) => value,
            None => panic!("Failed to parse Amount from string"),
        }
    }

    /// Formats the amount as a decimal string with the specified number of decimal places.
    ///
    /// ## Panics
    /// Panics if `decimals` is greater than 57.
    pub fn to_decimal_string(&self, decimals: u32) -> String {
        let mut s = String::new();
        self.fmt_decimals(&mut s, decimals)
            .expect("fmt with String is infallible");
        s
    }

    pub fn fmt_decimals<F: fmt::Write>(&self, f: &mut F, decimals: u32) -> fmt::Result {
        if decimals == 0 {
            write!(f, "{}", self.inner_value())?;
            return Ok(());
        }

        // i128 can represent up to ~10^38, so 38 decimal places is a safe upper bound
        if decimals > 38 {
            return Err(fmt::Error::custom("Too many decimal places"));
        }

        let divisor = 10u128.pow(decimals);
        let integer_part = self.inner_value() / divisor;
        let fractional_part = self.inner_value() % divisor;

        // Format fractional part with leading zeros
        write!(f, "{}.", integer_part)?;

        // TODO: calculate the decimal string without allocating a string first
        let fractional_str = fractional_part.to_string();
        let mut padding_needed = decimals as usize - fractional_str.len();
        while padding_needed > 0 {
            write!(f, "0")?;
            padding_needed -= 1;
        }
        write!(f, "{}", fractional_part)
    }

    #[cfg(feature = "precision")]
    pub fn into_precision_amount(self) -> crate::precision::PrecisionAmount {
        crate::precision::PrecisionAmount::from_integer(self.into_inner_value())
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.inner_value(), f)
    }
}

impl FromStr for Amount {
    type Err = num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = u128::from_str(s)?;
        Ok(Self(value))
    }
}

impl Default for Amount {
    fn default() -> Self {
        Self::zero()
    }
}

impl ops::Add<u64> for Amount {
    type Output = Self;

    fn add(self, other: u64) -> Self::Output {
        self + Amount::from_u64(other)
    }
}

impl ops::Sub<u64> for Amount {
    type Output = Self;

    fn sub(self, other: u64) -> Self::Output {
        self - Amount::from_u64(other)
    }
}

impl ops::Mul<u64> for Amount {
    type Output = Self;

    fn mul(self, other: u64) -> Self::Output {
        self * Amount::from_u64(other)
    }
}

impl ops::Div<u64> for Amount {
    type Output = Self;

    fn div(self, other: u64) -> Self::Output {
        self / Amount::from_u64(other)
    }
}

impl_from!(Amount, u8);
impl_try_from!(Amount, i8);
impl_try_from!(Amount, i16);
impl_from!(Amount, u16);
impl_try_from!(Amount, i32);
impl_from!(Amount, u32);
impl_from!(Amount, u64);
impl_try_from!(Amount, i64);
impl_from!(Amount, u128);
impl_try_from!(Amount, i128);
impl_try_from!(Amount, usize);
impl_try_from!(Amount, isize);

impl TryFrom<Amount> for usize {
    type Error = num::TryFromIntError;

    fn try_from(value: Amount) -> Result<Self, Self::Error> {
        value.into_inner_value().try_into()
    }
}

impl PartialOrd<usize> for Amount {
    fn partial_cmp(&self, other: &usize) -> Option<cmp::Ordering> {
        if self.is_negative() {
            return Some(cmp::Ordering::Less);
        }
        match usize::try_from(self.into_inner_value()) {
            Ok(value) => Some(value.cmp(other)),
            Err(_) => Some(cmp::Ordering::Greater),
        }
    }
}

partial_eq_impl!(Amount, u8);
partial_eq_impl!(Amount, i8);
partial_eq_impl!(Amount, i16);
partial_eq_impl!(Amount, u16);
partial_eq_impl!(Amount, i32);
partial_eq_impl!(Amount, u32);
partial_eq_impl!(Amount, i64);
partial_eq_impl!(Amount, u64);
partial_eq_impl!(Amount, u128);
partial_eq_impl!(Amount, i128);
partial_eq_impl!(Amount, usize);
partial_eq_impl!(Amount, isize);

partial_ord_impl!(Amount, u8);
partial_ord_impl!(Amount, i8);
partial_ord_impl!(Amount, u16);
partial_ord_impl!(Amount, i16);
partial_ord_impl!(Amount, u32);
partial_ord_impl!(Amount, i32);
partial_ord_impl!(Amount, u64);
partial_ord_impl!(Amount, i64);
partial_ord_impl!(Amount, u128);
partial_ord_impl!(Amount, i128);

#[cfg(feature = "borsh")]
mod borsh_impl {
    use borsh::{BorshSerialize, io};
    impl BorshSerialize for super::Amount {
        fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
            self.inner_value().serialize(writer)
        }
    }
}

impl Sum for Amount {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Self(iter.map(|a| a.into_inner_value()).sum())
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use serde_json::json;

    use super::*;

    #[test]
    fn basic_arithmetic() {
        let a = Amount::from_u64(4);
        let b = Amount::from_u64(6);
        let c = a + b;
        assert_eq!(c, 10i64);
        let d = a.saturating_sub(b);
        assert_eq!(d, 0);
        let e = a * b;
        assert_eq!(e, 24i64);
        let f = b / a;
        assert_eq!(f, 1i64);
    }

    #[test]
    fn checked_arithmetic() {
        let a = Amount::from_u64(4);
        let b = Amount::from_u64(6);
        let c = a.checked_add(b).unwrap();
        assert_eq!(c, Amount::from_u64(10));
        let e = a.checked_mul(b).unwrap();
        assert_eq!(e, Amount::from_u64(24));
        let f = b.checked_div(a).unwrap();
        assert_eq!(f, Amount::from_u64(1));
        let g = Amount::from_u64(7);
        let h = g.checked_div_ceil(Amount::from_u64(2)).unwrap();
        assert_eq!(h, 4);
        let i = Amount::from_u64(8);
        let j = i.checked_pow(3).unwrap();
        assert_eq!(j, Amount::from_u64(512));

        // Test overflow
        let max = Amount::MAX;
        let overflow_add = max.checked_add(Amount::from_u64(1));
        assert!(overflow_add.is_none(), "Overflow should return None");
        let overflow_sub = Amount::MIN.checked_sub(Amount::from_u64(1));
        assert!(overflow_sub.is_none(), "Underflow should return None");
        let overflow_mul = max.checked_mul(Amount::from_u64(2));
        assert!(overflow_mul.is_none(), "Overflow should return None");
        let overflow_div = Amount::ONE.checked_div(Amount::zero());
        assert!(overflow_div.is_none(), "Division by zero should return None");
        let overflow_div_ceil = Amount::ONE.checked_div_ceil(Amount::zero());
        assert!(overflow_div_ceil.is_none(), "Division by zero should return None");
        let overflow_pow = max.checked_pow(10);
        assert!(overflow_pow.is_none(), "Overflow should return None");
    }

    #[test]
    fn to_and_from_u64() {
        let a = Amount::from_u64(1234567890);
        assert_eq!(a, 1234567890);
        let a = Amount::from_u64(u64::MAX);
        assert_eq!(a, u64::MAX);
    }

    #[test]
    fn saturating_arithmetic() {
        let a = Amount::from_u64(4);
        let b = Amount::from_u64(6);
        let c = a.saturating_add(Amount::MAX);
        assert_eq!(c, Amount::MAX);
        let e = a.saturating_mul(Amount::MAX);
        assert_eq!(e, Amount::MAX);
        let f = b.saturating_div(&a);
        assert_eq!(f, Amount::from_u64(1));

        // Test saturating overflow
        let max = Amount::MAX;
        let overflow_add = max.saturating_add(Amount::from_u64(1));
        assert_eq!(
            overflow_add,
            Amount::MAX,
            "Saturating add should return MAX on overflow"
        );
        let overflow_sub = Amount::MIN.saturating_sub(Amount::from_u64(1));
        assert_eq!(
            overflow_sub,
            Amount::MIN,
            "Saturating sub should return MIN on underflow"
        );
        let overflow_mul = max.saturating_mul(Amount::from_u64(2));
        assert_eq!(
            overflow_mul,
            Amount::MAX,
            "Saturating mul should return MAX on overflow"
        );
    }

    #[test]
    #[cfg(feature = "extra-arith")]
    fn extra_arithmetic() {
        let k = Amount::from_u64(27);
        let l = k.checked_sqrt().unwrap();
        assert_eq!(l, Amount::from_u64(5));
    }

    #[test]
    fn can_serialize() {
        let a = Amount::from_u64(4);
        let b = serde_json::to_value(a).unwrap();
        assert_eq!(b, json!("4"));
    }

    #[test]
    fn can_de_serialize_using_cbor() {
        let a = Amount::MAX;
        let encoded = tari_bor::encode(&a).unwrap();
        let decoded = tari_bor::decode::<Amount>(&encoded).unwrap();
        assert_eq!(a, decoded);
        let a = Amount::MIN;
        let encoded = tari_bor::encode(&a).unwrap();
        let decoded = tari_bor::decode::<Amount>(&encoded).unwrap();
        assert_eq!(a, decoded);
    }

    #[test]
    fn to_from_str() {
        let a = Amount::from(u128::MAX);
        let s = a.to_string();
        assert_eq!(s, u128::MAX.to_string());

        let b: Amount = s.parse().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn to_le_bytes() {
        let a = Amount::from(u128::MAX - 1);
        let bytes = a.to_le_bytes();
        let expected_bytes = (u128::MAX - 1).to_le_bytes();
        assert_eq!(expected_bytes, bytes);
    }

    #[test]
    fn u64_ord() {
        let a = Amount::from_u64(4);
        let b = Amount::from_u64(6);
        assert!(a < b);
        assert!(b > a);
        assert!(a <= b);
        assert!(b >= a);
    }

    #[test]
    fn consts() {
        const N: Amount = Amount::from_str_radix("12345678901234567890", 10);
        assert_eq!(N, Amount::from(12345678901234567890u128));
    }

    #[test]
    fn fmt_decimals() {
        let a = Amount::from(123456u64);
        assert_eq!(a.to_decimal_string(0), "123456");
        assert_eq!(a.to_decimal_string(2), "1234.56");
        assert_eq!(a.to_decimal_string(5), "1.23456");
        assert_eq!(a.to_decimal_string(6), "0.123456");
        assert_eq!(a.to_decimal_string(8), "0.00123456");

        assert_eq!(a.to_decimal_string(38), "0.00000000000000000000000000000000123456");

        // > 57 decimals errors
        let mut s = String::new();
        a.fmt_decimals(&mut s, 39).unwrap_err();
    }

    mod from_le_slice {
        use super::*;

        #[test]
        fn exact_length() {
            let bytes = [1u8; Amount::BYTE_SIZE];
            let amount = Amount::from_le_slice(&bytes).unwrap();
            assert_eq!(amount.to_le_bytes(), bytes);
        }

        #[test]
        fn shorter_length() {
            let bytes = [1u8; 8];
            let amount = Amount::from_le_slice(&bytes).unwrap();
            let mut expected_bytes = [0u8; Amount::BYTE_SIZE];
            expected_bytes[..8].copy_from_slice(&bytes);
            assert_eq!(amount.to_le_bytes(), expected_bytes);
            assert_eq!(amount, 0x0101010101010101u64);
        }

        #[test]
        fn longer_length_with_trailing_zeros() {
            let mut bytes = vec![1u8; Amount::BYTE_SIZE - 5];
            bytes.extend(vec![0u8; 10]);
            let amount = Amount::from_le_slice(&bytes).unwrap();
            let mut expected_bytes = [0u8; Amount::BYTE_SIZE];
            expected_bytes[..(Amount::BYTE_SIZE - 5)].copy_from_slice(&bytes[..(Amount::BYTE_SIZE - 5)]);
            assert_eq!(amount.to_le_bytes(), expected_bytes);
        }

        #[test]
        fn longer_length_without_trailing_zeros() {
            let bytes = vec![1u8; Amount::BYTE_SIZE + 5];
            let amount = Amount::from_le_slice(&bytes);
            assert!(amount.is_none());
        }
    }
}
