//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

#[cfg(feature = "crypto")]
mod crypto;

/// Defines a conversion from a type to its light-weight byte representation.
pub trait ToByteType {
    type ByteType;
    fn to_byte_type(&self) -> Self::ByteType;
}

pub trait ConvertFromByteType<T> {
    type Error;

    fn convert_from_byte_type(bytes: &T) -> Result<Self, Self::Error>
    where Self: Sized;
}

pub trait FromByteType<T>: Sized {
    type Error;

    fn try_from_byte_type(&self) -> Result<T, Self::Error>;
}

impl<T: ConvertFromByteType<B>, B> FromByteType<T> for B {
    type Error = T::Error;

    fn try_from_byte_type(&self) -> Result<T, Self::Error> {
        T::convert_from_byte_type(self)
    }
}

impl<T: ToByteType> ToByteType for Option<T> {
    type ByteType = Option<T::ByteType>;

    fn to_byte_type(&self) -> Self::ByteType {
        self.as_ref().map(|v| v.to_byte_type())
    }
}
