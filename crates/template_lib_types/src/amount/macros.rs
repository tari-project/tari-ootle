//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

//! Implementation macros for this crate's two amount types. They are `#[macro_export]`ed because that is how a
//! `macro_rules!` reaches the crate's other modules, not because they are callable from outside it: half of
//! them expand to calls on a private accessor. `#[doc(hidden)]` is the label that says so.

/// Macro to implement `From` trait for a type that can be constructed from an integer.
#[doc(hidden)]
#[macro_export]
macro_rules! impl_from {
    ($ty:ty, $int:ty) => {
        impl From<$int> for $ty {
            fn from(value: $int) -> Self {
                Self::new(value.into())
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! impl_try_from {
    ($ty:ty, $int:ty) => {
        impl TryFrom<$int> for $ty {
            type Error = tari_template_abi::rust::num::TryFromIntError;

            fn try_from(value: $int) -> Result<Self, Self::Error> {
                Ok(Self(value.try_into()?))
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! partial_eq_impl {
    ($ty:ty, $other:ty) => {
        impl PartialEq<$other> for $ty {
            fn eq(&self, other: &$other) -> bool {
                let converted: Option<$other> = self.into_inner_value().try_into().ok();
                converted == Some(*other)
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! partial_ord_impl {
    ($ty:ty, $other:ty) => {
        impl PartialOrd<$other> for $ty {
            fn partial_cmp(&self, other: &$other) -> Option<tari_template_abi::rust::cmp::Ordering> {
                use tari_template_abi::rust::cmp;
                match <$other>::try_from(self.into_inner_value()) {
                    Ok(value) => value.partial_cmp(other),
                    Err(_) => {
                        if self.is_negative() {
                            Some(cmp::Ordering::Less)
                        } else {
                            Some(cmp::Ordering::Greater)
                        }
                    },
                }
            }
        }
    };
}

/// Implements a binary `core::ops` operator for an amount type in terms of its checked counterpart, so that an
/// unrepresentable result panics rather than wrapping. The panic messages are the ones Rust emits for the
/// primitive operators, because these operators are meant to behave as those do.
///
/// An amount type has to make that check itself. `overflow-checks` is a profile setting that does not travel
/// with the published crate or with a template author's own build, and it has no effect at all on `bnum`, which
/// backs `PrecisionAmount` and gates its own check on `debug_assertions`.
///
/// A divisor takes the second form, which names the by-zero case separately: the checked operator answers
/// `None` to both, and the two are not the same mistake.
#[doc(hidden)]
#[macro_export]
macro_rules! op_impl {
    ($item:ident, $trt:ident, $method:ident, $checked:ident, $overflow:literal) => {
        impl tari_template_abi::rust::ops::$trt for $item {
            type Output = $item;

            fn $method(self, other: $item) -> $item {
                match self.$checked(other) {
                    Some(value) => value,
                    None => panic!($overflow),
                }
            }
        }
    };
    ($item:ident, $trt:ident, $method:ident, $checked:ident, $overflow:literal, $by_zero:literal) => {
        impl tari_template_abi::rust::ops::$trt for $item {
            type Output = $item;

            fn $method(self, other: $item) -> $item {
                if other.is_zero() {
                    panic!($by_zero);
                }
                match self.$checked(other) {
                    Some(value) => value,
                    None => panic!($overflow),
                }
            }
        }
    };
}

/// Implements a compound-assignment `core::ops` operator for an amount type in terms of the binary operator
/// [`op_impl`] generated, so both reject the same results.
#[doc(hidden)]
#[macro_export]
macro_rules! op_assign_impl {
    ($item:ident, $trt:ident, $method:ident, $op_trt:ident, $op_method:ident) => {
        impl tari_template_abi::rust::ops::$trt for $item {
            fn $method(&mut self, other: $item) {
                use tari_template_abi::rust::ops::$op_trt;
                *self = (*self).$op_method(other);
            }
        }
    };
}
