//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[macro_export]
macro_rules! resource_address {
    ($s:expr) => {
        $crate::macros::_macro_exports::ResourceAddress::from_hex($s).expect("Failed to parse resource string")
    };
}

pub mod _macro_exports {
    pub use tari_template_lib_types::ResourceAddress;
}

/// A macro to create a `NonZeroU64` constant from a literal expression.
/// Panics at compile time if the value is zero.
#[macro_export]
macro_rules! const_nonzero_u64 {
    ($val:expr) => {{
        const __NONZERO: core::num::NonZeroU64 = core::num::NonZeroU64::new($val).expect("Value must be non-zero");
        __NONZERO
    }};
}

#[cfg(test)]
mod tests {

    #[test]
    fn it_generates_a_non_zero() {
        // let nz = const_nonzero_u64!(5-5); // This line would not compile
        const NZ: core::num::NonZeroU64 = const_nonzero_u64!(5);
        assert_eq!(NZ.get(), 5);
    }
}
