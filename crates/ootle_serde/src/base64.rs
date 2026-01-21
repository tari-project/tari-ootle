//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Deserializer, Serializer};

use crate::visitor::BytesVisitor;

pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
    if s.is_human_readable() {
        let base64 = STANDARD.encode(v);
        s.serialize_str(&base64)
    } else {
        s.serialize_bytes(v.as_ref())
    }
}

pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    for<'a> T: TryFrom<Vec<u8>>,
{
    if d.is_human_readable() {
        let s = String::deserialize(d)?;
        let bytes = STANDARD.decode(s.as_bytes()).map_err(serde::de::Error::custom)?;
        T::try_from(bytes).map_err(|_| {
            serde::de::Error::custom(format!(
                "base64 Failed to convert bytes to {}",
                std::any::type_name::<T>()
            ))
        })
    } else {
        let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
        T::try_from(bytes.into()).map_err(|_| {
            serde::de::Error::custom(format!(
                "base64: Failed to convert base64 bytes to {}",
                std::any::type_name::<T>()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use tari_template_lib::types::Hash;

    use super::*;

    #[derive(Deserialize, Serialize, PartialEq, Debug)]
    struct SampleData {
        #[serde(with = "super")]
        data: Vec<u8>,
        #[serde(with = "super")]
        hash: Hash,
    }

    #[test]
    fn it_encodes_and_decodes_from_base64() {
        let original = SampleData {
            data: vec![1, 2, 3, 4, 5],
            hash: Hash::from_array([123u8; 32]),
        };

        // Serialize to JSON (human-readable)
        let json = serde_json::to_value(&original).unwrap();
        json["data"].as_str().expect("data should be a string");
        json["hash"].as_str().expect("hash should be a string");

        // Deserialize from JSON
        let deserialized: SampleData = serde_json::from_value(json).unwrap();
        assert_eq!(original, deserialized);

        // Serialize to binary (non-human-readable)
        let binary = bincode::serde::encode_to_vec(&original, bincode::config::standard()).unwrap();

        // Deserialize from binary
        let (deserialized_bin, _) =
            bincode::serde::decode_from_slice::<SampleData, _>(&binary, bincode::config::standard()).unwrap();
        assert_eq!(original, deserialized_bin);
    }
}
