//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

mod amount;
// mod macros;
mod ops;
#[cfg(feature = "serde")]
mod serde;

pub use amount::*;

pub mod public_macros {
    /// Creates a constant `Amount` from a string literal representing a base-10 integer
    /// at compile time.
    ///
    /// # Examples
    /// ```rust,ignore
    /// # use crate::models::{precision_amount, PrecisionAmount};
    /// const AMOUNT: PrecisionAmount = precision_amount!("1234567890");
    /// assert_eq!(AMOUNT, PrecisionAmount::from(1234567890u64));
    /// const NEGATIVE_AMOUNT: PrecisionAmount = precision_amount!("-1234567890");
    ///  assert_eq!(NEGATIVE_AMOUNT, PrecisionAmount::from(-1234567890));
    ///  ```
    #[macro_export]
    macro_rules! precision_amount {
        ($int:expr) => {{ $crate::precision::PrecisionAmount::from_str_radix($int, 10) }};
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSITIVE: PrecisionAmount = crate::precision_amount!("1234567890");
    const NEGATIVE: PrecisionAmount = crate::precision_amount!("-1234567890");
    #[test]
    fn consts() {
        assert_eq!(POSITIVE, PrecisionAmount::from(1234567890u64));
        assert_eq!(NEGATIVE, PrecisionAmount::from(-1234567890));
    }
}
