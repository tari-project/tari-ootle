//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::{fmt, prelude::*};

use crate::KeyParseError;

pub fn fixed_bytes_from_hex<const L: usize>(s: &str) -> Result<[u8; L], KeyParseError> {
    let s = s.as_bytes();
    if s.len() != L * 2 {
        return Err(KeyParseError);
    }

    let mut bytes = [0u8; L];
    for (byte, chunk) in bytes.iter_mut().zip(s.chunks_exact(2)) {
        *byte = (hex_digit(chunk[0])? << 4) | hex_digit(chunk[1])?;
    }
    Ok(bytes)
}

pub fn bytes_from_hex(s: &str) -> Result<Vec<u8>, KeyParseError> {
    let s = s.as_bytes();
    if !s.len().is_multiple_of(2) {
        return Err(KeyParseError);
    }

    let mut bytes = Vec::with_capacity(s.len() / 2);
    for chunk in s.chunks_exact(2) {
        bytes.push((hex_digit(chunk[0])? << 4) | hex_digit(chunk[1])?);
    }
    Ok(bytes)
}

fn hex_digit(b: u8) -> Result<u8, KeyParseError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(KeyParseError),
    }
}

pub fn bytes_to_hex<T: AsRef<[u8]>>(bytes: T) -> String {
    let mut hex = String::with_capacity(bytes.as_ref().len() * 2);
    write_hex_fmt(&mut hex, bytes.as_ref()).expect("String write infaiible");
    hex
}

pub fn write_hex_fmt<W: fmt::Write>(writer: &mut W, bytes: &[u8]) -> fmt::Result {
    for b in bytes {
        write!(writer, "{:02x}", b)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_formats_as_hex() {
        let bytes = [0xde, 0xad, 0xbe, 0xef];
        let hex = bytes_to_hex(bytes);
        assert_eq!(hex, "deadbeef");
    }

    #[test]
    fn it_decodes_valid_hex() {
        assert_eq!(fixed_bytes_from_hex::<4>("deadbeef").unwrap(), [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(fixed_bytes_from_hex::<4>("DEADBEEF").unwrap(), [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(bytes_from_hex("deadbeef").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn it_rejects_wrong_length() {
        assert!(fixed_bytes_from_hex::<4>("deadbe").is_err());
        assert!(fixed_bytes_from_hex::<4>("deadbeef00").is_err());
        assert!(bytes_from_hex("abc").is_err());
    }

    #[test]
    fn it_rejects_non_hex_without_panicking() {
        assert!(fixed_bytes_from_hex::<2>("zzzz").is_err());
        assert!(bytes_from_hex("gg").is_err());
        // A leading '+' is valid for u8::from_str_radix but is not a valid hex byte.
        assert!(bytes_from_hex("+a").is_err());
    }

    #[test]
    fn it_rejects_multibyte_utf8_without_panicking() {
        // Regression: the length guard counts bytes, so a multibyte char positioned to
        // straddle a 2-byte chunk boundary used to slice mid-codepoint and panic.
        // `é` (2 bytes) at byte offset 1 makes byte index 2 a non-char-boundary.
        let mut s = String::from("a");
        s.push('é');
        s.push_str(&"a".repeat(61));
        assert_eq!(s.len(), 64);
        assert!(fixed_bytes_from_hex::<32>(&s).is_err());

        // 4-byte emoji with even byte-length passes the parity guard in bytes_from_hex.
        assert!(bytes_from_hex("😀").is_err());
        assert!(bytes_from_hex(&"😀".repeat(16)).is_err());
    }
}
