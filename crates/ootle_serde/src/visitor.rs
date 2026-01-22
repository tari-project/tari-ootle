//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, marker::PhantomData};

use serde::{de, de::Visitor};

#[derive(Default)]
pub struct BytesVisitor<'a>(PhantomData<&'a ()>);

impl<'a> BytesVisitor<'a> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<'a> Visitor<'a> for BytesVisitor<'a> {
    type Value = BytesCow<'a>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("bytes (in template_lib)")
    }

    fn visit_borrowed_bytes<E>(self, v: &'a [u8]) -> Result<Self::Value, E>
    where E: de::Error {
        Ok(BytesCow::Borrowed(v))
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where E: de::Error {
        Ok(BytesCow::Owned(v.into_boxed_slice()))
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where E: de::Error {
        Ok(BytesCow::Owned(v.to_vec().into_boxed_slice()))
    }
}

// Cow is not available in no_std, so we define our own
pub enum BytesCow<'a> {
    Borrowed(&'a [u8]),
    Owned(Box<[u8]>),
}

impl<'a> BytesCow<'a> {
    pub const fn len(&self) -> usize {
        match self {
            BytesCow::Borrowed(v) => v.len(),
            BytesCow::Owned(v) => v.len(),
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl BytesCow<'_> {
    pub fn into_owned(self) -> Box<[u8]> {
        match self {
            BytesCow::Borrowed(v) => v.to_vec().into_boxed_slice(),
            BytesCow::Owned(v) => v,
        }
    }
}

impl<'a> From<BytesCow<'a>> for Vec<u8> {
    fn from(value: BytesCow<'a>) -> Self {
        match value {
            BytesCow::Borrowed(v) => v.to_vec(),
            BytesCow::Owned(v) => v.into_vec(),
        }
    }
}

impl<'a> From<BytesCow<'a>> for Box<[u8]> {
    fn from(value: BytesCow<'a>) -> Self {
        value.into_owned()
    }
}

impl<'a> AsRef<[u8]> for BytesCow<'a> {
    fn as_ref(&self) -> &[u8] {
        match self {
            BytesCow::Borrowed(v) => v,
            BytesCow::Owned(v) => v.as_ref(),
        }
    }
}
