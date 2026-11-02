//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use alloc::{boxed::Box, vec::Vec};
use core::{fmt, marker::PhantomData};

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

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where A: de::SeqAccess<'a> {
        let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1024));
        while let Some(byte) = seq.next_element::<u8>()? {
            bytes.push(byte);
        }
        Ok(BytesCow::Owned(bytes.into_boxed_slice()))
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

#[cfg(test)]
mod tests {
    use serde::Deserializer;

    use super::*;

    #[test]
    fn it_reads_bytes_from_a_sequence() {
        // serde_json answers `deserialize_byte_buf` on an array with `visit_seq`
        let mut de = serde_json::Deserializer::from_str("[1, 2, 255]");
        let bytes = (&mut de).deserialize_byte_buf(BytesVisitor::new()).unwrap();
        assert_eq!(bytes.as_ref(), [1, 2, 255]);
    }

    #[test]
    fn it_rejects_a_sequence_element_above_u8() {
        let mut de = serde_json::Deserializer::from_str("[1, 256]");
        assert!((&mut de).deserialize_byte_buf(BytesVisitor::new()).is_err());
    }
}
