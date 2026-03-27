//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use serde_json as json;

const MAX_DEPTH: usize = 50;

#[derive(Debug, thiserror::Error)]
#[error("Maximum depth of {} exceeded during JSON to CBOR conversion", MAX_DEPTH)]
pub struct MaxDepthExceeded;

/// Direct conversion from a serde JSON value to a CBOR value type.
///
/// Returns an error if the maximum depth (50) is exceeded during conversion.
pub fn convert_json_to_cbor(value: json::Value) -> Result<tari_bor::Value, MaxDepthExceeded> {
    convert_json_to_cbor_inner(value, 0)
}

fn convert_json_to_cbor_inner(value: json::Value, depth: usize) -> Result<tari_bor::Value, MaxDepthExceeded> {
    if depth > MAX_DEPTH {
        return Err(MaxDepthExceeded);
    }
    match value {
        json::Value::Null => Ok(tari_bor::Value::Null),
        json::Value::Bool(v) => Ok(tari_bor::Value::Bool(v)),
        json::Value::Number(n) => Ok(n
            .as_i64()
            .map(|v| tari_bor::Value::Integer(v.into()))
            .or_else(|| n.as_f64().map(tari_bor::Value::Float))
            .expect("A JSON number is always convertable to an integer or a float")),
        // Allow special string parsing within nested arrays and objects
        json::Value::String(s) => Ok(tari_bor::Value::Text(s)),
        json::Value::Array(arr) => Ok(tari_bor::Value::Array(
            arr.into_iter()
                .map(|a| convert_json_to_cbor_inner(a, depth + 1))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        json::Value::Object(map) => Ok(tari_bor::Value::Map(
            map.into_iter()
                .map(|(k, v)| convert_json_to_cbor_inner(v, depth + 1).map(|v| (tari_bor::Value::Text(k), v)))
                .collect::<Result<_, _>>()?,
        )),
    }
}
