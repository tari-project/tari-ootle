//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

/// Macro to implement `From` trait for a type that can be constructed from an integer.
#[macro_export]
macro_rules! impl_from {
    ($ty:ty, $int:ty) => {
        impl From<$int> for $ty {
            fn from(value: $int) -> Self {
                Self::new(value)
            }
        }
    };
}

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

#[macro_export]
macro_rules! op_impl {
    ($item: ident, $trt:ident, $method:ident) => {
        impl tari_template_abi::rust::ops::$trt for $item {
            type Output = $item;

            fn $method(self, other: $item) -> $item {
                $item::new(self.0.$method(other.0))
            }
        }
    };
}
