//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! `serde(with = "ootle_serde::cbor_value")` adapter for fields of type
//! [`tari_bor::Value`].
//!
//! Previously this delegated to a wrapper that worked around ciborium's `Value` having a
//! lossy JSON encoding. With the new minicbor-based `Value` it ships its own
//! [`Serialize`]/[`Deserialize`] impls (sentinel-object based, round-trip safe), so this
//! adapter is now a trivial pass-through kept for backwards compatibility with existing
//! `#[serde(with = "...")]` annotations.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn serialize<S: Serializer>(v: &tari_bor::Value, s: S) -> Result<S::Ok, S::Error> {
    v.serialize(s)
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<tari_bor::Value, D::Error> {
    tari_bor::Value::deserialize(d)
}
