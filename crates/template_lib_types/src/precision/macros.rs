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
    ($ty:ty) => {
        impl PartialEq<$ty> for Amount {
            fn eq(&self, other: &$ty) -> bool {
                let converted: Option<$ty> = self.into_inner_value().try_into().ok();
                converted == Some(*other)
            }
        }
    };
}

#[macro_export]
macro_rules! partial_ord_impl {
    ($ty:ty) => {
        impl PartialOrd<$ty> for Amount {
            fn partial_cmp(&self, other: &$ty) -> Option<tari_template_abi::rust::cmp::Ordering> {
                use tari_template_abi::rust::cmp;
                match <$ty>::try_from(self.into_inner_value()) {
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
