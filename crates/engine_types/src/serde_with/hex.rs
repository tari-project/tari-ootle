//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::borrow::Cow;

use serde::{Deserialize, Deserializer, Serializer};
use tari_template_lib::types::serde_helpers::BytesVisitor;

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
                "Failed to convert bytes to {}: {e}",
                std::any::type_name::<T>()
            ))
        })?
    } else {
        let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
        T::try_from(bytes.as_ref()).map_err(|e| {
            serde::de::Error::custom(format!(
                "Failed to convert bytes to {}: {e}",
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
                "Failed to convert bytes to {}: {e}",
                std::any::type_name::<T>()
            ))
        })?
    } else {
        let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
        T::try_from(bytes.into()).map_err(|e| {
            serde::de::Error::custom(format!(
                "Failed to convert bytes to {}: {e}",
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
                    "Failed to convert bytes to {}: {e}",
                    std::any::type_name::<T>()
                ))
            })?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    #[derive(Serialize, Deserialize)]
    struct TestCase {
        #[serde(with = "super")]
        fixed: [u8; 32],
        #[serde(with = "super")]
        vec: Vec<u8>,
    }

    // Test it
    #[test]
    fn test_serialize() {
        let data = TestCase {
            fixed: [1; 32],
            vec: vec![5; 100],
        };
        let serialized = serde_json::to_vec(&data).unwrap();
        let deserialized: TestCase = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(data.fixed, deserialized.fixed);
        assert_eq!(data.vec, deserialized.vec);
    }
}
