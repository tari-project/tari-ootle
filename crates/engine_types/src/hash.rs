//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! `Hash64` is defined in `tari_template_lib_types` so templates can name it too — the intrinsic
//! hashing API returns one — and is re-exported here for the crates that reached it through this
//! path. Its encoding is unchanged: a CBOR byte string, not an array of integers.

pub use tari_template_lib::types::{Hash64, HashParseError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_deserialize() {
        let hash = Hash64::default();
        let mut buf = Vec::new();
        tari_bor::encode_into_writer(&hash, &mut buf).unwrap();
        let val = tari_bor::to_value(&hash).unwrap();
        assert_eq!(val, tari_bor::Value::Bytes(vec![0u8; Hash64::LENGTH]));
        let hash2 = tari_bor::decode(&buf).unwrap();
        assert_eq!(hash, hash2);
    }
}
