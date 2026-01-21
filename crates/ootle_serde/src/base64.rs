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
        T::try_from(bytes)
            .map_err(|_| serde::de::Error::custom(format!("Failed to convert bytes to {}", std::any::type_name::<T>())))
    } else {
        let bytes = d.deserialize_byte_buf(BytesVisitor::new())?;
        T::try_from(bytes.into())
            .map_err(|_| serde::de::Error::custom(format!("Failed to convert bytes to {}", std::any::type_name::<T>())))
    }
}
