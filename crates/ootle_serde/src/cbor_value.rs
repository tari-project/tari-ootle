//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tari_bor::json_encoding::{CborValueJsonSerializeWrapper, CiboruimValueDeserializeFixWrapper};

pub fn serialize<S: Serializer>(v: &tari_bor::Value, s: S) -> Result<S::Ok, S::Error> {
    if s.is_human_readable() {
        // This uses a wrapper type to serialize the CBOR value to a JSON-compatible format
        // We cannot use this wrapper for non-human-readable formats because then the resulting CBOR would be would not
        // be valid CBOR binary data
        CborValueJsonSerializeWrapper(v).serialize(s)
    } else {
        #[cfg(feature = "bincode-compat")]
        {
            // This is to support bincode since the default ciborium serde implementation uses deserialize_any instead
            // // of serializing the cbor::Value enum directly
            // - unfortunately, when using CBOR, Value::Bytes will be used instead of the actual CBOR representation.
            // NOTE: this increases fees (see FeeModule::on_before_finalize)
            // Other solutions include:
            // - switching to cbor4ii, implementing to_value and from_value and a serializer that supports
            //   deserialize_any
            // - storing a Vec<u8> and incurring the extra encode/decode steps (what this does)
            let vec = tari_bor::encode(v).map_err(serde::ser::Error::custom)?;
            s.serialize_bytes(&vec)
        }
        #[cfg(not(feature = "bincode-compat"))]
        v.serialize(s)
    }
}

pub fn deserialize<'de, D>(d: D) -> Result<tari_bor::Value, D::Error>
where D: Deserializer<'de> {
    if d.is_human_readable() {
        let wrapper = CiboruimValueDeserializeFixWrapper::deserialize(d)?;
        Ok(wrapper.0)
    } else {
        #[cfg(feature = "bincode-compat")]
        {
            use crate::visitor::BytesVisitor;
            let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
            tari_bor::decode_exact(bytes.as_ref()).map_err(serde::de::Error::custom)
        }
        #[cfg(not(feature = "bincode-compat"))]
        tari_bor::Value::deserialize(d)
    }
}

#[cfg(test)]
mod tests {
    use tari_bor::cbor;
    use tari_template_lib::types::{ObjectKey, ResourceAddress};

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Test {
        #[serde(with = "super")]
        value: tari_bor::Value,
    }

    #[test]
    fn decode_encode_json() {
        let addr = ResourceAddress::new([1u8; ObjectKey::LENGTH].into());
        let test = Test {
            value: cbor!({
                "code" => 415,
                "message" => addr,
                "continue" => false,
                "array" => [1, 2, 3, 4, 5],
                "extra" => { "numbers" => [8.2341e+4, 0.251425] },
            })
            .unwrap(),
        };

        let json = serde_json::to_string(&test).unwrap();
        let t = serde_json::from_str::<Test>(&json).unwrap();
        assert_eq!(test, t);

        let json2 = serde_json::to_string(&t).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn decode_encode_cbor() {
        let addr = ResourceAddress::new([1u8; ObjectKey::LENGTH].into());
        let test = Test {
            value: cbor!({
                "code" => 415,
                "message" => addr,
                "continue" => false,
                "array" => [1, 2, 3, 4, 5],
                "extra" => { "numbers" => [8.2341e+4, 0.251425] },
            })
            .unwrap(),
        };

        let bytes = tari_bor::encode(&test).unwrap();
        let t = tari_bor::decode_exact::<Test>(&bytes).unwrap();
        assert_eq!(test, t);

        let json2 = tari_bor::encode(&t).unwrap();
        assert_eq!(bytes, json2);
    }

    #[test]
    #[cfg(feature = "bincode-compat")]
    fn decode_encode_bincode() {
        let addr = ResourceAddress::new([1u8; ObjectKey::LENGTH].into());
        let test = Test {
            value: cbor!({
                "code" => 415,
                "message" => addr,
                "continue" => false,
                "array" => [1, 2, 3, 4, 5],
                "extra" => { "numbers" => [8.2341e+4, 0.251425] },
            })
            .unwrap(),
        };

        let bytes = bincode::serde::encode_to_vec(&test, bincode::config::standard()).unwrap();
        let (t, _) = bincode::serde::decode_from_slice::<Test, _>(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(test, t);

        let bytes2 = bincode::serde::encode_to_vec(&t, bincode::config::standard()).unwrap();
        assert_eq!(bytes, bytes2);
    }
}
