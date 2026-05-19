//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::any::type_name;

use serde::Serialize;
use tari_ootle_storage::StorageError;

// Function names retain the `_bincode` suffix to avoid churning a few dozen call sites;
// on the wire the indexer now stores minicbor (via minicbor-serde) instead. bincode v2's
// `deserialize_any` ban broke round-trips for any value containing tari_bor::Value, so
// switching the indexer's sqlite blob format follows the same fix landed for the
// rocksdb state store.
pub fn serialize_bincode<T: Serialize + ?Sized>(t: &T) -> Result<Vec<u8>, StorageError> {
    minicbor_serde::to_vec(t).map_err(|e| StorageError::EncodingError {
        operation: "serialize_bincode",
        item: type_name::<T>(),
        details: e.to_string(),
    })
}

pub fn deserialize_bincode<T: serde::de::DeserializeOwned, S: AsRef<[u8]>>(s: S) -> Result<T, StorageError> {
    minicbor_serde::from_slice(s.as_ref()).map_err(|e| StorageError::DecodingError {
        operation: "deserialize_bincode",
        item: type_name::<T>(),
        details: e.to_string(),
    })
}

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
