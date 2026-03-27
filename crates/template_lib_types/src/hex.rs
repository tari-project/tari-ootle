//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::{fmt, prelude::*};

use crate::KeyParseError;

pub fn fixed_bytes_from_hex<const L: usize>(s: &str) -> Result<[u8; L], KeyParseError> {
    if s.len() != L * 2 {
        return Err(KeyParseError);
    }

    let mut bytes = [0u8; L];
    for (i, h) in bytes.iter_mut().enumerate() {
        *h = u8::from_str_radix(&s[2 * i..2 * (i + 1)], 16).map_err(|_| KeyParseError)?;
    }
    Ok(bytes)
}

pub fn bytes_from_hex(s: &str) -> Result<Vec<u8>, KeyParseError> {
    if !s.len().is_multiple_of(2) {
        return Err(KeyParseError);
    }

    let mut bytes = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| KeyParseError)?;
        bytes.push(byte);
    }
    Ok(bytes)
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
}
