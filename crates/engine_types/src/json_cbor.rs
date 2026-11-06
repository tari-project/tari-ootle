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
            .or_else(|| n.as_u64().map(|v| tari_bor::Value::Integer(v.into())))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_signed_and_unsigned_64_bit_integers_without_precision_loss() {
        let min_i64 = convert_json_to_cbor(json::json!(i64::MIN)).unwrap();
        assert_eq!(min_i64, tari_bor::Value::Integer(i128::from(i64::MIN)));

        let max_i64 = convert_json_to_cbor(json::json!(i64::MAX)).unwrap();
        assert_eq!(max_i64, tari_bor::Value::Integer(i128::from(i64::MAX)));

        let above_i64_max = (i64::MAX as u64) + 1;
        let cbor_above = convert_json_to_cbor(json::json!(above_i64_max)).unwrap();
        assert_eq!(cbor_above, tari_bor::Value::Integer(i128::from(above_i64_max)));

        let max_u64 = convert_json_to_cbor(json::json!(u64::MAX)).unwrap();
        assert_eq!(max_u64, tari_bor::Value::Integer(i128::from(u64::MAX)));
        let decoded: u64 = tari_bor::from_value(&max_u64).unwrap();
        assert_eq!(decoded, u64::MAX);

        let float_val = convert_json_to_cbor(json::json!(1.5)).unwrap();
        assert_eq!(float_val, tari_bor::Value::Float(1.5));
    }
}
