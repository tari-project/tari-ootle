//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

//! Serialize `Amount` using as a string for human-readable formats. For deserialisation we allow numbers or strings,
//! however the format has to support deserialize_any (serde_json does)
//!
//! For non-human-readable formats, we serialize as little-endian u64 digits which is slightly more compact and
//! efficient than the bnum implementation (which simply uses the derive macro).

use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{Error, SeqAccess},
};
use tari_template_abi::rust::{fmt, prelude::*};

use super::PrecisionAmount as Amount;

impl Serialize for Amount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        if serializer.is_human_readable() {
            // We always serialize as a string in JSON for arbitrary precision
            serializer.serialize_str(&self.to_string())
        } else {
            self.to_le_digits().serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        use tari_template_abi::rust::str::FromStr;
        struct AmountVisitor;

        impl<'de> serde::de::Visitor<'de> for AmountVisitor {
            type Value = Amount;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string or an integer representing a Amount")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where E: Error {
                Amount::from_str(value).map_err(Error::custom)
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where E: Error {
                Amount::from_str(&value).map_err(Error::custom)
            }

            fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
            where E: Error {
                Ok(Amount::from(v))
            }

            fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
            where E: Error {
                Ok(Amount::from(v))
            }

            fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
            where E: Error {
                Ok(Amount::from(v))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where E: Error {
                Ok(Amount::from(v))
            }

            fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E>
            where E: Error {
                Ok(Amount::from(v))
            }

            fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
            where E: Error {
                Ok(Amount::from(v))
            }

            fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
            where E: Error {
                Ok(Amount::from(v))
            }

            fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
            where E: Error {
                Ok(Amount::from(v))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where E: Error {
                Ok(Amount::from(value))
            }

            fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
            where E: Error {
                Ok(Amount::from(value))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where A: SeqAccess<'de> {
                let digit1 = seq
                    .next_element::<u64>()?
                    .ok_or_else(|| Error::invalid_length(0, &self))?;
                let digit2 = seq
                    .next_element::<u64>()?
                    .ok_or_else(|| Error::invalid_length(1, &self))?;
                let digit3 = seq
                    .next_element::<u64>()?
                    .ok_or_else(|| Error::invalid_length(2, &self))?;
                Ok(Amount::from_le_digits([digit1, digit2, digit3]))
            }
        }

        // Precision-Amount accepts a number, a string, a digit array, or raw bytes — driven by the actual shape
        // via deserialize_any. Both JSON and minicbor-serde support this; bincode does not, but bincode is no
        // longer part of the storage stack.
        deserializer.deserialize_any(AmountVisitor)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tari_bor::{decode_exact, encode};

    use super::*;

    #[test]
    fn json_encoding_decoding() {
        let amount = Amount::from(12345678901234567890u128);
        let json = serde_json::to_string(&amount).unwrap();
        assert_eq!(json, "\"12345678901234567890\"");

        let decoded: Amount = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, amount);

        // it deserializes from a number
        let amount = Amount::from(123u64);
        let val = json!(123u32);
        let decoded: Amount = serde_json::from_value(val).unwrap();
        assert_eq!(decoded, amount);
    }

    #[test]
    fn cbor_encoding_decoding() {
        let amount = -Amount::from(12345678901234567890u128);
        let cbor = encode(&amount).unwrap();
        let decoded: Amount = decode_exact(&cbor).unwrap();
        assert_eq!(decoded, amount);

        // Raw format
        let cbor = encode(&[123u64, 0, 0]).unwrap();
        let decoded: Amount = decode_exact(&cbor).unwrap();
        assert_eq!(decoded, 123);

        // Decode directly from a number.
        let amount = 123i32;
        let cbor = encode(&amount).unwrap();
        let decoded: Amount = decode_exact(&cbor).unwrap();
        assert_eq!(decoded, amount);
        let amount = -1234567890i64;
        let cbor = encode(&amount).unwrap();
        let decoded: Amount = decode_exact(&cbor).unwrap();
        assert_eq!(decoded, amount);
    }
}
