//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use bnum::BUint;
use newtype_ops::newtype_ops;
use serde::ser::Error;
use tari_template_abi::rust::{cmp, fmt, fmt::Debug, iter::Sum, ops::Neg, str::FromStr, write};

use crate::{impl_from, partial_eq_impl, partial_ord_impl};

/// A 192-bit signed integer type.
pub type I192 = bnum::BInt<3>; // 3 x 64 bits = 192 bits

/// A 192-bit signed amount.
///
/// This is a general purpose signed integer, but is primarily used to represent the smallest unit of value in
/// resources/vaults etc.
///
/// This allows Tari to support a massive number tokens within resources.
/// e.g. 2 ETH = 2 x 10^18 Gwei.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Amount(#[cfg_attr(feature = "ts", ts(type = "string | number"))] pub(super) I192);

impl Amount {
    pub const BITS: usize = I192::BITS as usize;
    pub const BYTE_SIZE: usize = I192::BYTES as usize;
    /// The maximum value that an `Amount` can hold.
    pub const MAX: Self = Self::new(I192::MAX);
    /// The minimum value that an `Amount` can hold.
    pub const MIN: Self = Self::new(I192::MIN);
    /// The number of u64 digits used to represent a Amount i.e. 3
    pub const NUM_DIGITS: usize = Self::BITS / u64::BITS as usize;
    /// The value of one, represented as an `Amount`.
    pub const ONE: Self = Self::new(I192::ONE);
    /// The value of one hundred, represented as an `Amount`.
    pub const ONE_HUNDRED: Self = Self::from_le_digits([100, 0, 0]);
    /// The value of one thousand, represented as an `Amount`.
    pub const ONE_THOUSAND: Self = Self::from_le_digits([1000, 0, 0]);
    /// The value of ten, represented as an `Amount`.
    pub const TEN: Self = Self::from_le_digits([10, 0, 0]);
    /// The value of zero, represented as an `Amount`.
    pub const ZERO: Self = Self::new(I192::ZERO);
    /// The number of u64 digits used to represent a Amount i.e. 3
    /// This is internal but needs to be public for macros
    pub const _U64_BYTES: usize = (u64::BITS / 8) as usize;
    /// The number of bytes used to represent a single u64 digit in the Amount.
    /// This is internal but needs to be public for macros
    pub const _U64_BYTE_SHIFT: usize = Self::_U64_BYTES.trailing_zeros() as usize;

    /// Creates a new `Amount` from an `i64` value.
    pub const fn new(amount: I192) -> Self {
        Self(amount)
    }

    /// A value of zero.
    pub const fn zero() -> Self {
        Self::ZERO
    }

    /// Returns true if the amount is zero.
    pub const fn is_zero(&self) -> bool {
        self.inner_value().is_zero()
    }

    /// Returns true if the amount is positive (greater than zero).
    pub const fn is_positive(&self) -> bool {
        self.inner_value().is_positive()
    }

    /// Returns true if the amount is non-negative (greater than or equal to zero).
    pub const fn is_non_negative(&self) -> bool {
        self.is_positive() || self.is_zero()
    }

    /// Returns true if the amount is negative (less than zero).
    pub const fn is_negative(&self) -> bool {
        self.inner_value().is_negative()
    }

    /// Returns the inner value of this amount as an `I192`.
    const fn inner_value(&self) -> &I192 {
        &self.0
    }

    const fn into_inner_value(self) -> I192 {
        self.0
    }

    /// Returns the value of this amount + other. Returns `None` if the result underflows or overflows.
    pub const fn checked_add(&self, other: Self) -> Option<Self> {
        // match is to allow const function
        match self.inner_value().checked_add(other.into_inner_value()) {
            Some(value) => Some(Self::new(value)),
            None => None,
        }
    }

    /// Returns the sum of two amounts, returning `None` if either value is negative or if the result overflows.
    pub const fn checked_add_positive(&self, other: Self) -> Option<Self> {
        if self.is_negative() || other.is_negative() {
            return None;
        }
        self.checked_add(other)
    }

    /// Returns the sum of two amounts, saturating at `i64::MAX` if the result exceeds it.
    pub const fn saturating_add(&self, other: Self) -> Self {
        Self::new(self.into_inner_value().saturating_add(other.into_inner_value()))
    }

    /// Returns the difference of two amounts, saturating at `0` if the result is negative.
    pub const fn checked_sub(&self, other: Self) -> Option<Self> {
        match self.into_inner_value().checked_sub(other.into_inner_value()) {
            Some(value) => Some(Self::new(value)),
            None => None,
        }
    }

    pub fn sum_from_positive<I: Iterator<Item = Self>>(iter: I) -> Option<Self> {
        let mut sum = Self::zero();
        for amount in iter {
            sum = sum.checked_add_positive(amount)?;
        }
        Some(sum)
    }

    /// Returns the difference of two amounts, saturating at `0` if the result is negative.
    pub const fn saturating_sub(&self, other: Self) -> Self {
        Self::new(self.into_inner_value().saturating_sub(other.into_inner_value()))
    }

    /// Returns the difference of two amounts, returning `None` if the result is negative.
    pub fn saturating_sub_positive(&self, other: Self) -> Option<Self> {
        if *self < other {
            return None;
        }

        Some(Self::new(self.inner_value() - other.inner_value()))
    }

    /// Returns the difference of two amounts, returning `None` if the result is negative or if either amount is
    /// negative.
    pub fn checked_sub_positive(&self, other: Self) -> Option<Self> {
        if self.is_negative() || other.is_negative() {
            return None;
        }
        self.saturating_sub_positive(other)
    }

    /// Returns the product of two amounts, returning `None` if the result overflows.
    pub const fn checked_mul(&self, other: &Self) -> Option<Self> {
        match self.into_inner_value().checked_mul(other.into_inner_value()) {
            Some(value) => Some(Self::new(value)),
            None => None,
        }
    }

    /// Returns the product of two amounts, saturating at `i64::MAX` if the result exceeds it.
    pub const fn saturating_mul(&self, other: &Self) -> Self {
        Self::new(self.into_inner_value().saturating_mul(other.into_inner_value()))
    }

    /// Returns the quotient of two amounts, returning `None` if the divisor is zero or if the result overflows.
    pub const fn checked_div(&self, other: Self) -> Option<Self> {
        match self.into_inner_value().checked_div(other.into_inner_value()) {
            Some(value) => Some(Self::new(value)),
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
        if rem.is_zero() || div.is_negative() {
            Some(Self::new(div))
        } else {
            // Otherwise, we round up
            Some(Self::new(div.add(I192::ONE)))
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
        Self::new(self.into_inner_value().saturating_div(other.into_inner_value()))
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
        // to_le_bytes requires nightly, because they need some trait related const features.
        // We implement it manually, copying almost verbatim (except we define some consts ourselves)

        let value_bits = self.inner_value().to_bits();
        // Strangely, this is slightly faster than direct transmutation by either `mem::transmute_copy` or `ptr::read`.
        // Also, initialising the bytes with zeros is faster than using MaybeUninit.
        // The Rust compiler is probably being very smart and optimizing this code.
        // The same goes for `to_be_bytes`.
        let mut bytes = [0; Self::BYTE_SIZE];
        let mut i = 0;
        while i < Self::NUM_DIGITS {
            let digit_bytes = value_bits.digits()[i].to_le_bytes();
            let mut j = 0;
            while j < Self::_U64_BYTES {
                bytes[(i << Self::_U64_BYTE_SHIFT) + j] = digit_bytes[j];
                j += 1;
            }
            i += 1;
        }
        bytes
    }

    pub fn from_le_bytes(bytes: [u8; Self::BYTE_SIZE]) -> Self {
        // from_le_bytes requires nightly, because they need some trait related const features.
        // We implement it manually, copying almost verbatim

        let mut out = [0u64; Self::NUM_DIGITS];
        let mut i = 0;
        while i < Self::NUM_DIGITS {
            let mut digit_bytes = [0u8; Self::_U64_BYTES];
            let init_index = i << Self::_U64_BYTE_SHIFT;
            let mut j = init_index;
            while j < init_index + Self::_U64_BYTES {
                digit_bytes[j - init_index] = bytes[j];
                j += 1;
            }
            out[i] = u64::from_le_bytes(digit_bytes);
            i += 1;
        }

        Self::from_le_digits(out)
    }

    pub const fn from_le_digits(digits: [u64; Self::NUM_DIGITS]) -> Self {
        let out = I192::from_bits(BUint::<3>::from_digits(digits));
        Self::new(out)
    }

    pub fn from_le_slice(bytes: &[u8]) -> Option<Self> {
        I192::from_le_slice(bytes).map(Self::new)
    }

    pub fn to_le_digits(&self) -> [u64; Self::NUM_DIGITS] {
        *self.inner_value().to_bits().digits()
    }

    /// If the amount is negative (< 0), returns `None`, otherwise returns `Some(self)`.
    pub fn non_negative_checked(self) -> Option<Self> {
        if self.is_negative() {
            None
        } else {
            Some(self)
        }
    }

    /// If the amount is positive (> 0), returns `None`, otherwise returns `Some(self)`.
    pub fn negative_checked(self) -> Option<Self> {
        if self.is_positive() {
            None
        } else {
            Some(self)
        }
    }

    /// Parses a string as an `Amount` in the specified radix.
    ///
    /// This function works in constant context, allowing it to be used to define constants.
    pub const fn try_from_str_radix(s: &str, radix: u32) -> Option<Self> {
        match I192::from_str_radix(s, radix) {
            Ok(value) => Some(Self::new(value)),
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

        // I192 can represent up to ~10^57, so 57 decimal places is a safe upper bound
        if decimals > 57 {
            return Err(fmt::Error::custom("Too many decimal places"));
        }

        let ten = I192::from(10);
        let divisor = ten.pow(decimals);
        let integer_part = self.inner_value().div(divisor);
        let fractional_part = self.inner_value().rem(divisor).abs();

        if self.is_negative() && integer_part.is_zero() && !fractional_part.is_zero() {
            write!(f, "-")?;
        }

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
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.inner_value(), f)
    }
}

impl FromStr for Amount {
    type Err = bnum::errors::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = I192::from_str_radix(s, 10)?;
        Ok(Self::new(value))
    }
}

impl Default for Amount {
    fn default() -> Self {
        Self::zero()
    }
}

newtype_ops! { [Amount] {add sub mul div} {:=} Self Self }
newtype_ops! { [Amount] {add sub mul div} {:=} &Self &Self }

impl_from!(Amount, u8);
impl_from!(Amount, i8);
impl_from!(Amount, i16);
impl_from!(Amount, u16);
impl_from!(Amount, i32);
impl_from!(Amount, u32);
impl_from!(Amount, u64);
impl_from!(Amount, i64);
impl_from!(Amount, u128);
impl_from!(Amount, i128);
impl_from!(Amount, usize);
impl_from!(Amount, isize);

// impl From<Amount> for Amount {
//     fn from(value: Amount) -> Self {
//         Self::new(value.as_signed_value().into())
//     }
// }
//
// impl TryFrom<Amount> for Amount {
//     type Error = bnum::errors::TryFromIntError;
//
//     fn try_from(value: Amount) -> Result<Self, Self::Error> {
//         Ok(Amount::new(value.into_inner_value().try_into()?))
//     }
// }

impl TryFrom<Amount> for usize {
    type Error = bnum::errors::TryFromIntError;

    fn try_from(value: Amount) -> Result<Self, Self::Error> {
        value.into_inner_value().try_into()
    }
}

impl Neg for Amount {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::new(-self.inner_value())
    }
}

// impl PartialEq<Amount> for Amount {
//     fn eq(&self, other: &Amount) -> bool {
//         self.into_inner_value()
//             .try_into()
//             .ok()
//             .map(|n: i64| n == other.as_signed_value())
//             .unwrap_or(false)
//     }
// }
//
// impl PartialOrd<Amount> for Amount {
//     fn partial_cmp(&self, other: &Amount) -> Option<cmp::Ordering> {
//         if self.is_negative() {
//             return Some(cmp::Ordering::Less);
//         }
//         match self.into_inner_value().try_into().map(Amount::new) {
//             Ok(value) => Some(value.cmp(other)),
//             Err(_) => Some(cmp::Ordering::Greater),
//         }
//     }
// }

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

partial_eq_impl!(u8);
partial_eq_impl!(i8);
partial_eq_impl!(i16);
partial_eq_impl!(u16);
partial_eq_impl!(i32);
partial_eq_impl!(u32);
partial_eq_impl!(i64);
partial_eq_impl!(u64);
partial_eq_impl!(u128);
partial_eq_impl!(i128);
partial_eq_impl!(usize);
partial_eq_impl!(isize);

partial_ord_impl!(u8);
partial_ord_impl!(i8);
partial_ord_impl!(u16);
partial_ord_impl!(i16);
partial_ord_impl!(u32);
partial_ord_impl!(i32);
partial_ord_impl!(u64);
partial_ord_impl!(i64);
partial_ord_impl!(u128);
partial_ord_impl!(i128);

#[cfg(feature = "borsh")]
mod borsh_impl {
    use borsh::BorshSerialize;
    impl BorshSerialize for super::Amount {
        fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
            self.inner_value().serialize(writer)
        }
    }
}

impl Sum for Amount {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Self::new(iter.map(|a| a.into_inner_value()).sum())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn basic_arithmetic() {
        let a = Amount::from(4);
        let b = Amount::from(6);
        let c = a + b;
        assert_eq!(c, 10i64);
        let d = a - b;
        assert_eq!(d, -2i64);
        let e = a * b;
        assert_eq!(e, 24i64);
        let f = b / a;
        assert_eq!(f, 1i64);
    }

    #[test]
    fn checked_arithmetic() {
        let a = Amount::from(4);
        let b = Amount::from(6);
        let c = a.checked_add(b).unwrap();
        assert_eq!(c, Amount::from(10));
        let d = a.checked_sub(b).unwrap();
        assert_eq!(d, Amount::from(-2));
        let e = a.checked_mul(&b).unwrap();
        assert_eq!(e, Amount::from(24));
        let f = b.checked_div(a).unwrap();
        assert_eq!(f, Amount::from(1));

        // Test overflow
        let max = Amount::MAX;
        let overflow_add = max.checked_add(Amount::from(1));
        assert!(overflow_add.is_none(), "Overflow should return None");
        let overflow_sub = Amount::MIN.checked_sub(Amount::from(1));
        assert!(overflow_sub.is_none(), "Underflow should return None");
        let overflow_mul = max.checked_mul(&Amount::from(2));
        assert!(overflow_mul.is_none(), "Overflow should return None");
        let overflow_div = Amount::from(1).checked_div(Amount::zero());
        assert!(overflow_div.is_none(), "Division by zero should return None");
        let overflow_div_ceil = Amount::from(1).checked_div_ceil(Amount::zero());
        assert!(overflow_div_ceil.is_none(), "Division by zero should return None");
    }

    #[test]
    fn can_serialize() {
        let a = Amount::from(4);
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

        // Test negative value
        let a = -Amount::from(u128::MAX);
        let s = a.to_string();
        assert_eq!(s, format!("-{}", u128::MAX));

        let b: Amount = s.parse().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn to_le_bytes() {
        let a = Amount::from(u128::MAX - 1);
        let bytes = a.to_le_bytes();
        let mut expected_bytes = (u128::MAX - 1).to_le_bytes().to_vec();
        expected_bytes.extend([0u8; 8]);
        assert_eq!(expected_bytes, bytes);

        let a = Amount::from(-i128::MAX);
        let bytes = a.to_le_bytes();
        let b = Amount::from_le_bytes(bytes);
        assert_eq!(a, b);
    }

    #[test]
    fn u64_ord() {
        let a = Amount::from(4);
        let b = Amount::from(6);
        assert!(a < b);
        assert!(b > a);
        assert!(a <= b);
        assert!(b >= a);

        // Negatives
        let c = Amount::from(-4);
        let d = Amount::from(6);
        assert!(c < d);
        assert!(d > c);
        assert!(c <= d);
        assert!(d >= c);
    }

    #[test]
    fn consts() {
        const N: Amount = Amount::from_str_radix("12345678901234567890", 10);
        assert_eq!(N, Amount::from(12345678901234567890u128));
        const N2: Amount = Amount::from_str_radix("-12345678901234567890", 10);
        assert_eq!(N2, Amount::from(-12345678901234567890i128));
    }

    #[test]
    fn fmt_decimals() {
        let a = Amount::from(123456);
        assert_eq!(a.to_decimal_string(0), "123456");
        assert_eq!(a.to_decimal_string(2), "1234.56");
        assert_eq!(a.to_decimal_string(5), "1.23456");
        assert_eq!(a.to_decimal_string(6), "0.123456");
        assert_eq!(a.to_decimal_string(8), "0.00123456");

        assert_eq!(
            a.to_decimal_string(57),
            "0.000000000000000000000000000000000000000000000000000123456"
        );

        // > 57 decimals errors
        let mut s = String::new();
        a.fmt_decimals(&mut s, 58).unwrap_err();

        let b = Amount::from(-123456);
        assert_eq!(b.to_decimal_string(0), "-123456");
        assert_eq!(b.to_decimal_string(2), "-1234.56");
        assert_eq!(b.to_decimal_string(5), "-1.23456");
        assert_eq!(b.to_decimal_string(6), "-0.123456");
        assert_eq!(b.to_decimal_string(8), "-0.00123456");

        let c = Amount::from(1000);
        assert_eq!(c.to_decimal_string(3), "1.000");
        assert_eq!(c.to_decimal_string(5), "0.01000");

        let c = Amount::from(-1000);
        assert_eq!(c.to_decimal_string(3), "-1.000");
        assert_eq!(c.to_decimal_string(8), "-0.00001000");
    }
}
