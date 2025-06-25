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

pub fn deserialize_json<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, StorageError> {
    serde_json::from_str(s).map_err(|e| StorageError::DecodingError {
        operation: "deserialize_json",
        item: type_name::<T>(),
        details: e.to_string(),
    })
}

pub fn serialize_hex<T: AsRef<[u8]>>(bytes: T) -> String {
    hex::encode(bytes.as_ref())
}
