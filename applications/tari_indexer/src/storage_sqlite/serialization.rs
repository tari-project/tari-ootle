//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::any::type_name;

use serde::Serialize;
use tari_ootle_storage::StorageError;

pub fn serialize_json<T: Serialize + ?Sized>(t: &T) -> Result<String, StorageError> {
    serde_json::to_string(t).map_err(|e| StorageError::EncodingError {
        operation: "serialize_json",
        item: type_name::<T>(),
        details: e.to_string(),
    })
}

pub fn deserialize_json<T: serde::de::DeserializeOwned, S: AsRef<str>>(s: S) -> Result<T, StorageError> {
    serde_json::from_str(s.as_ref()).map_err(|e| StorageError::DecodingError {
        operation: "deserialize_json",
        item: type_name::<T>(),
        details: e.to_string(),
    })
}

pub fn serialize_hex<T: AsRef<[u8]>>(bytes: T) -> String {
    hex::encode(bytes.as_ref())
}

fn deserialize_hex(s: &str) -> Result<Vec<u8>, StorageError> {
    hex::decode(s).map_err(|e| StorageError::DecodingError {
        operation: "deserialize_hex",
        item: "Vec<u8>",
        details: e.to_string(),
    })
}

pub fn deserialize_hex_try_from<T, U: AsRef<str>>(s: U) -> Result<T, StorageError>
where
    for<'a> T: TryFrom<&'a [u8]>,
    for<'a> <T as TryFrom<&'a [u8]>>::Error: std::fmt::Display,
{
    let bytes = deserialize_hex(s.as_ref())?;
    T::try_from(&bytes).map_err(|e| StorageError::DecodingError {
        operation: "deserialize_hex_try_from",
        item: type_name::<T>(),
        details: e.to_string(),
    })
}
