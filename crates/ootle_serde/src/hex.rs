//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::borrow::Cow;

use serde::{Deserialize, Deserializer, Serializer};

use crate::visitor::BytesVisitor;

pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
    if s.is_human_readable() {
        let st = hex::encode(v.as_ref());
        s.serialize_str(&st)
    } else {
        s.serialize_bytes(v.as_ref())
    }
}

pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: for<'a> TryFrom<&'a [u8]>,
    for<'a> <T as TryFrom<&'a [u8]>>::Error: std::fmt::Display,
{
    let value = if d.is_human_readable() {
        let hex = <Cow<'_, str> as Deserialize>::deserialize(d)?;
        let bytes = hex::decode(&*hex).map_err(serde::de::Error::custom)?;
        T::try_from(&bytes).map_err(|e| {
            serde::de::Error::custom(format!(
                "Failed to convert hex bytes to {}: {e}",
                std::any::type_name::<T>()
            ))
        })?
    } else {
        let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
        T::try_from(bytes.as_ref()).map_err(|e| {
            serde::de::Error::custom(format!(
                "Failed to convert hex bytes to {}: {e}",
                std::any::type_name::<T>()
            ))
        })?
    };

    Ok(value)
}

/// Use this if T owns the bytes
pub fn deserialize_from_vec<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: TryFrom<Vec<u8>>,
    T::Error: std::fmt::Display,
{
    let value = if d.is_human_readable() {
        let hex = <Cow<'_, str> as Deserialize>::deserialize(d)?;
        let bytes = hex::decode(&*hex).map_err(serde::de::Error::custom)?;
        T::try_from(bytes).map_err(|e| {
            serde::de::Error::custom(format!(
                "Failed to convert hex bytes to {}: {e}",
                std::any::type_name::<T>()
            ))
        })?
    } else {
        let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
        T::try_from(bytes.into()).map_err(|e| {
            serde::de::Error::custom(format!(
                "Failed to convert hex bytes to {}: {e}",
                std::any::type_name::<T>()
            ))
        })?
    };

    Ok(value)
}

pub mod option {
    use super::*;

    pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &Option<T>, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            match v {
                Some(v) => {
                    let st = hex::encode(v.as_ref());
                    s.serialize_some(&st)
                },
                None => s.serialize_none(),
            }
        } else {
            match v {
                Some(v) => s.serialize_some(v.as_ref()),
                None => s.serialize_none(),
            }
        }
    }

    pub fn deserialize<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: for<'a> TryFrom<&'a [u8]>,
        for<'a> <T as TryFrom<&'a [u8]>>::Error: std::fmt::Display,
    {
        let bytes = if d.is_human_readable() {
            let hex = <Option<String> as Deserialize>::deserialize(d)?;
            hex.as_ref()
                .map(hex::decode)
                .transpose()
                .map_err(serde::de::Error::custom)?
        } else {
            <Option<Vec<u8>> as Deserialize>::deserialize(d)?
        };

        let value = bytes
            .as_ref()
            .map(|b| T::try_from(b.as_slice()))
            .transpose()
            .map_err(|e| {
                serde::de::Error::custom(format!(
                    "Failed to convert hex bytes to {}: {e}",
                    std::any::type_name::<T>()
                ))
            })?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use tari_template_lib::types::Hash32;

    use super::*;

    #[derive(Serialize, Deserialize)]
    struct TestCase {
        #[serde(with = "super")]
        fixed: [u8; 32],
        #[serde(with = "super")]
        vec: Vec<u8>,
        #[serde(with = "super")]
        hash: Hash32,
    }

    // Test it
    #[test]
    fn test_serialize() {
        let data = TestCase {
            fixed: [1; 32],
            vec: vec![5; 100],
            hash: Hash32::from_array([2; 32]),
        };
        let serialized = serde_json::to_vec(&data).unwrap();
        let deserialized: TestCase = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(data.fixed, deserialized.fixed);
        assert_eq!(data.vec, deserialized.vec);
        assert_eq!(data.hash, deserialized.hash);
    }
}
