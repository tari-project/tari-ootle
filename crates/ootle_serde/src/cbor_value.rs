//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tari_bor::cbor_value_encoding_fix::{CborValueDeserializeFixWrapper, CborValueSerializeFixWrapper};

pub fn serialize<S: Serializer>(v: &tari_bor::Value, s: S) -> Result<S::Ok, S::Error> {
    if cfg!(feature = "bincode-compat") || s.is_human_readable() {
        // This uses a wrapper type to serialize the CBOR value to a JSON and bincode-compatible format
        // We cannot use this wrapper for cbor encoding itself (non-human readable) because then the result
        // would be would not be valid CBOR binary data.
        CborValueSerializeFixWrapper(v).serialize(s)
    } else {
        v.serialize(s)
    }
}

pub fn deserialize<'de, D>(d: D) -> Result<tari_bor::Value, D::Error>
where D: Deserializer<'de> {
    if cfg!(feature = "bincode-compat") || d.is_human_readable() {
        let wrapper = CborValueDeserializeFixWrapper::deserialize(d)?;
        Ok(wrapper.0)
    } else {
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

        // These assertions only work with bincode-compat disabled
        // The check that the resulting Value is the CBOR of the Test struct, not the
        // Value enum itself encoded as CBOR
        #[cfg(not(feature = "bincode-compat"))]
        {
            use tari_bor::Tagged;
            let value = tari_bor::to_value(&test).unwrap();
            let (k, v) = value.as_map().expect("expected map").get(0).expect("expected value");
            assert_eq!(k.as_text().expect("expected string"), "value");

            let (k, v) = v.as_map().expect("expected map").get(1).expect("expected message");
            assert_eq!(k.as_text().expect("expected string"), "message");
            let (t, v) = v.as_tag().expect("expected resource tag");
            assert_eq!(t, ResourceAddress::TAG);
            v.as_bytes().expect("expected bytes");
            let t = tari_bor::from_value::<Test>(&value).unwrap();
            assert_eq!(test, t);
        }

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
