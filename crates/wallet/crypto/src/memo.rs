//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{cmp, io};

use tari_template_lib::types::{MaxBytes, MaxString};

/// These are selected to be out of range of the Minotari memo field tags
/// See:
/// https://github.com/tari-project/tari/blob/221d715e2447e6ca33e2ebcba11e915d24edac15/base_layer/transaction_components/src/transaction_components/memo_field.rs
#[repr(u8)]
enum MemoTag {
    Message = 0x10,
    Bytes = 0x11,
}

impl MemoTag {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x10 => Some(Self::Message),
            0x11 => Some(Self::Bytes),
            _ => None,
        }
    }
}

const MAX_BYTES_LENGTH: usize = 253;
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Memo {
    /// UTF-8 encoded string message
    Message(#[cfg_attr(feature = "ts", ts(type = "string"))] MaxString<MAX_BYTES_LENGTH>),
    /// Arbitrary bytes
    Bytes(#[cfg_attr(feature = "ts", ts(type = "string"))] MaxBytes<MAX_BYTES_LENGTH>),
}

impl Memo {
    /// EncryptedData memo size (255) - 2 (enum tag + length (u8))
    pub const MAX_BYTES_LENGTH: usize = MAX_BYTES_LENGTH;

    pub fn new_message(s: impl Into<Box<str>>) -> Option<Self> {
        let s = s.into();
        let s = MaxString::new_checked(s)?;
        Some(Self::Message(s))
    }

    pub fn new_bytes(b: impl Into<Box<[u8]>>) -> Option<Self> {
        let b = b.into();
        let b = MaxBytes::new_checked(b)?;
        Some(Self::Bytes(b))
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Memo::Message(s) => s.as_bytes(),
            Memo::Bytes(b) => b.as_ref(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Memo::Message(s) => s.len(),
            Memo::Bytes(b) => b.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn as_message(&self) -> Option<&str> {
        match self {
            Memo::Message(s) => Some(s),
            Memo::Bytes(_) => None,
        }
    }

    // We implement a custom encoding (that **almost** matches borsh). Because we are very byte constrained and since we
    // know that the max length is 253 bytes we can use a single byte for the length. Borsh uses a u32 for length (3
    // extra bytes of overhead).

    pub fn encode_into<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        let len = self.len();
        let len = u8::try_from(len).expect("len <= MAX_BYTES_LENGTH <= 255");
        match self {
            Self::Message(s) => {
                writer.write_all(&[MemoTag::Message as u8])?;
                writer.write_all(&[len])?;
                writer.write_all(s.as_bytes())?;
            },
            Self::Bytes(b) => {
                writer.write_all(&[MemoTag::Bytes as u8])?;
                writer.write_all(&[len])?;
                writer.write_all(b.as_ref())?;
            },
        }
        Ok(())
    }

    pub fn decode_from<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let mut tag = [0u8; 1];
        reader.read_exact(&mut tag)?;
        let tag = MemoTag::from_u8(tag[0]);

        let Some(tag) = tag else {
            // Fallback for future versions, preserve unknown memos as bytes (up to the max size)
            let mut buf = vec![0u8; Self::MAX_BYTES_LENGTH];
            let bytes_read = read_until_len_or_eof(&mut buf, reader, Self::MAX_BYTES_LENGTH)?;
            buf.truncate(bytes_read);
            return Ok(Self::new_bytes(buf).expect("length checked"));
        };

        let mut len_buf = [0u8; 1];
        reader.read_exact(&mut len_buf)?;
        let len = len_buf[0] as usize;
        if len > Self::MAX_BYTES_LENGTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Memo length {} exceeds maximum {}", len, Self::MAX_BYTES_LENGTH),
            ));
        }
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf)?;
        match tag {
            MemoTag::Message => {
                let s =
                    String::from_utf8(buf).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8"))?;
                Ok(Self::new_message(s).expect("length checked (buf.len() <= MAX_BYTES_LENGTH)"))
            },
            MemoTag::Bytes => Ok(Self::new_bytes(buf).expect("length checked (buf.len() <= MAX_BYTES_LENGTH)")),
        }
    }
}

fn read_until_len_or_eof<R: io::Read>(mut buf: &mut [u8], reader: &mut R, max_len: usize) -> io::Result<usize> {
    let mut bytes_read = 0;
    while !buf.is_empty() {
        match reader.read(buf) {
            Ok(0) => break,
            Ok(n) => {
                // Subtraction is safe because bytes_read <= max_len
                let bytes_to_take = cmp::min(n, max_len - bytes_read);
                bytes_read += bytes_to_take;
                // Copied from std::io::Read::read_exact but added the following to break if we reach max_len
                if bytes_read == max_len {
                    break;
                }

                buf = &mut buf[bytes_to_take..];
            },
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {},
            Err(e) => return Err(e),
        }
    }

    Ok(bytes_read)
}

#[cfg(test)]
mod tests {
    use tari_template_lib::types::EncryptedData;

    use super::*;

    #[test]
    fn it_returns_none_if_max_bytes_len_exceeded() {
        let bytes = vec![0u8; Memo::MAX_BYTES_LENGTH + 1];
        let memo = Memo::new_bytes(bytes);
        assert!(memo.is_none());
    }

    #[test]
    fn it_encodes_and_decodes() {
        let original = Memo::new_message("Hello, world!").unwrap();
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);

        let original = Memo::new_bytes(vec![1, 2, 3, 4, 5]).unwrap();
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);

        // Empty memo
        let original = Memo::new_message("").unwrap();
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);

        // Max size memo
        let bytes = vec![0u8; Memo::MAX_BYTES_LENGTH];
        let original = Memo::new_bytes(bytes).unwrap();
        let mut buf = Vec::new();
        original.encode_into(&mut buf).unwrap();
        let decoded = Memo::decode_from(&mut buf.as_slice()).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn it_fails_to_decode_if_length_is_too_big() {
        // THis is valid encoding but the length exceeds the max allowed length
        let encoded = [vec![MemoTag::Bytes as u8, (Memo::MAX_BYTES_LENGTH + 1) as u8], vec![
            1u8;
            Memo::MAX_BYTES_LENGTH +
                1
        ]]
        .concat();
        let _err = Memo::decode_from(&mut encoded.as_slice()).unwrap_err();
    }

    #[test]
    fn it_fails_to_decode_if_length_mismatches() {
        // This is invalid encoding as the length (10) exceeds the actual data length (5)
        let encoded = [vec![MemoTag::Message as u8, 10u8], vec![1u8; 5]].concat();
        let _err = Memo::decode_from(&mut encoded.as_slice()).unwrap_err();
    }

    #[test]
    fn it_falls_back_to_bytes_with_unknown_variants() {
        let encoded = [vec![0u8, 5], vec![1u8; 5]].concat();
        let decoded = Memo::decode_from(&mut encoded.as_slice()).unwrap();
        // Includes the length byte in the bytes since some future unknown variant may not be length prefixed
        assert_eq!(decoded, Memo::new_bytes(vec![5, 1, 1, 1, 1, 1]).unwrap());
    }

    #[test]
    fn it_borsh_encodes_to_max_bytes() {
        let bytes = vec![0u8; Memo::MAX_BYTES_LENGTH];
        let memo = Memo::new_bytes(bytes).unwrap();
        let mut buf = Vec::new();
        memo.encode_into(&mut buf).unwrap();
        // We want the encoded memo to fit in the max size of an EncryptedData payload (i.e 255 bytes)
        assert!(buf.len() <= EncryptedData::max_size() - EncryptedData::min_size());
    }
}
