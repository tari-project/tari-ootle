//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

#[allow(clippy::module_inception)]
mod amount;
mod macros;
mod serde;

pub use amount::*;
// Re-export errors
pub use bnum::errors::{ParseIntError, TryFromIntError};

pub mod public_macros {

    /// Creates a constant `Amount` from a string literal representing a base-10 integer
    /// at compile time.
    ///
    /// # Examples
    /// ```rust,ignore
    /// # use crate::models::{amount, Amount};
    /// const AMOUNT: Amount = amount!("1234567890");
    /// assert_eq!(AMOUNT, Amount::from(1234567890));
    /// const NEGATIVE_AMOUNT: Amount = amount!("-1234567890");
    ///  assert_eq!(NEGATIVE_AMOUNT, Amount::from(-1234567890));
    ///  ```
    #[macro_export]
    macro_rules! amount {
        ($int:expr) => {{
            $crate::Amount::from_str_radix($int, 10)
        }};
    }

    #[cfg(test)]
    mod tests {
        use crate::amount::Amount;

        const POSITIVE: Amount = amount!("1234567890");
        const NEGATIVE: Amount = amount!("-1234567890");
        #[test]
        fn consts() {
            assert_eq!(POSITIVE, Amount::from(1234567890));
            assert_eq!(NEGATIVE, Amount::from(-1234567890));
        }
    }
}
