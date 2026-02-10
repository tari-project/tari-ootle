//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

#[allow(clippy::module_inception)]
mod amount;
mod macros;
mod ops;
mod serde;

pub use amount::*;

pub mod public_macros {

    /// Creates a constant `Amount` from a string literal representing a base-10 integer
    /// at compile time.
    ///
    /// # Examples
    /// ```rust
    /// # use crate::models::{amount, Amount};
    /// const AMOUNT: Amount = amount!(1234567890);
    /// assert_eq!(AMOUNT, 1234567890);
    ///  ```
    #[macro_export]
    macro_rules! amount {
        ($int:expr) => {{ $crate::Amount::new($int as u128) }};
    }

    #[cfg(test)]
    mod tests {
        use crate::amount::Amount;

        const POSITIVE: Amount = amount!(1234567890);
        #[test]
        fn consts() {
            assert_eq!(POSITIVE, Amount::from(1234567890u64));
        }
    }
}
