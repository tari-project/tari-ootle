//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use alloc::{borrow::Cow, format, vec::Vec};
use core::{any, fmt::Display};

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
    for<'a> <T as TryFrom<&'a [u8]>>::Error: Display,
{
    let value = if d.is_human_readable() {
        let hex = <Cow<'_, str> as Deserialize>::deserialize(d)?;
        let bytes = hex::decode(&*hex).map_err(serde::de::Error::custom)?;
        T::try_from(&bytes).map_err(|e| {
            serde::de::Error::custom(format!("Failed to convert hex bytes to {}: {e}", any::type_name::<T>()))
        })?
    } else {
        let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
        T::try_from(bytes.as_ref()).map_err(|e| {
            serde::de::Error::custom(format!("Failed to convert hex bytes to {}: {e}", any::type_name::<T>()))
        })?
    };

    Ok(value)
}

/// Use this if T owns the bytes
pub fn deserialize_from_vec<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: TryFrom<Vec<u8>>,
    T::Error: Display,
{
    let value = if d.is_human_readable() {
        let hex = <Cow<'_, str> as Deserialize>::deserialize(d)?;
        let bytes = hex::decode(&*hex).map_err(serde::de::Error::custom)?;
        T::try_from(bytes).map_err(|e| {
            serde::de::Error::custom(format!("Failed to convert hex bytes to {}: {e}", any::type_name::<T>()))
        })?
    } else {
        let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
        T::try_from(bytes.into()).map_err(|e| {
            serde::de::Error::custom(format!("Failed to convert hex bytes to {}: {e}", any::type_name::<T>()))
        })?
    };

    Ok(value)
}

pub mod option {
    use serde::Serialize;

    use super::*;

    struct Encode<'a, T>(&'a T);

    impl<T: AsRef<[u8]>> Serialize for Encode<'_, T> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            super::serialize(self.0, s)
        }
    }

    struct Decode<T>(T);

    impl<'de, T> Deserialize<'de> for Decode<T>
    where
        T: for<'a> TryFrom<&'a [u8]>,
        for<'a> <T as TryFrom<&'a [u8]>>::Error: Display,
    {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            super::deserialize(d).map(Decode)
        }
    }

    pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &Option<T>, s: S) -> Result<S::Ok, S::Error> {
        v.as_ref().map(Encode).serialize(s)
    }

    pub fn deserialize<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: for<'a> TryFrom<&'a [u8]>,
        for<'a> <T as TryFrom<&'a [u8]>>::Error: Display,
    {
        Ok(Option::<Decode<T>>::deserialize(d)?.map(|Decode(v)| v))
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;
    use crate::test_fixtures::Bytes32;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct TestCase {
        #[serde(with = "super")]
        fixed: [u8; 32],
        #[serde(with = "super")]
        vec: Vec<u8>,
        #[serde(with = "super")]
        hash: Bytes32,
        #[serde(with = "super::option")]
        opt_hash: Option<Bytes32>,
    }

    // Test it
    #[test]
    fn test_serialize() {
        for opt_hash in [Some(Bytes32::new([3; 32])), None] {
            let data = TestCase {
                fixed: [1; 32],
                vec: vec![5; 100],
                hash: Bytes32::new([2; 32]),
                opt_hash,
            };
            let serialized = serde_json::to_vec(&data).unwrap();
            let deserialized: TestCase = serde_json::from_slice(&serialized).unwrap();
            assert_eq!(data, deserialized);

            let binary = tari_bor::serde_codec::to_vec(&data).unwrap();
            let deserialized_bin: TestCase = tari_bor::serde_codec::from_slice(&binary).unwrap();
            assert_eq!(data, deserialized_bin);
        }
    }

    #[derive(Serialize)]
    struct Required {
        #[serde(with = "super")]
        hash: Bytes32,
    }

    #[derive(Serialize)]
    struct Optional {
        #[serde(with = "super::option")]
        hash: Option<Bytes32>,
    }

    #[test]
    fn option_encodes_some_as_the_required_form() {
        let hash = Bytes32::new([7; 32]);
        let required = Required { hash };
        let optional = Optional { hash: Some(hash) };

        assert_eq!(
            tari_bor::serde_codec::to_vec(&optional).unwrap(),
            tari_bor::serde_codec::to_vec(&required).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&optional).unwrap(),
            serde_json::to_value(&required).unwrap()
        );
    }
}
