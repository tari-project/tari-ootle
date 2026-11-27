//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use serde_json as json;

const MAX_DEPTH: usize = 50;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum JsonCborError {
    #[error("Maximum depth of {MAX_DEPTH} exceeded during JSON to CBOR conversion")]
    MaxDepthExceeded,
    #[error("JSON number is outside the CBOR integer and finite f64 range")]
    NumberOutOfRange,
}

/// Direct conversion from a serde JSON value to a CBOR value type.
///
/// Returns an error if the maximum depth (50) is exceeded or if a number falls outside the CBOR
/// integer and finite `f64` range.
pub fn convert_json_to_cbor(value: json::Value) -> Result<tari_bor::Value, JsonCborError> {
    convert_json_to_cbor_inner(value, 0)
}

fn convert_json_to_cbor_inner(value: json::Value, depth: usize) -> Result<tari_bor::Value, JsonCborError> {
    if depth >= MAX_DEPTH {
        return Err(JsonCborError::MaxDepthExceeded);
    }
    match value {
        json::Value::Null => Ok(tari_bor::Value::Null),
        json::Value::Bool(v) => Ok(tari_bor::Value::Bool(v)),
        json::Value::Number(n) => convert_json_number(&n),
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

// Overflowing floats and integers outside [i64::MIN, u64::MAX] only reach `json::Number`
// when `serde_json/arbitrary_precision` is enabled (as in `tari_walletd`). Without that
// feature, `serde_json` rejects overflowing floats at parse time and widens integers above
// `u64::MAX` to `f64`.
fn convert_json_number(n: &json::Number) -> Result<tari_bor::Value, JsonCborError> {
    n.as_i64()
        .map(|v| tari_bor::Value::Integer(v.into()))
        .or_else(|| n.as_u64().map(|v| tari_bor::Value::Integer(v.into())))
        .or_else(|| n.as_f64().filter(|_| n.is_f64()).map(tari_bor::Value::Float))
        .ok_or(JsonCborError::NumberOutOfRange)
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

    #[test]
    fn rejects_numbers_outside_cbor_integer_and_finite_f64_range() {
        for raw in ["18446744073709551616", "-9223372036854775809", "1e400", "-1e400"] {
            let value: json::Value = json::from_str(raw).unwrap();
            assert_eq!(
                convert_json_to_cbor(value),
                Err(JsonCborError::NumberOutOfRange),
                "expected error for {raw}"
            );
        }
    }

    #[test]
    fn enforces_max_depth_consistent_with_indexed_value() {
        let mut allowed = json::json!(1);
        for _ in 0..(MAX_DEPTH - 1) {
            allowed = json::Value::Array(vec![allowed]);
        }
        let cbor = convert_json_to_cbor(allowed).expect("49 nested containers (50 value levels) should succeed");
        crate::indexed_value::IndexedValue::from_value(cbor)
            .expect("IndexedValue::from_value must accept every CBOR value produced by convert_json_to_cbor");

        let too_deep = (0..MAX_DEPTH).fold(json::json!(1), |acc, _| json::Value::Array(vec![acc]));
        assert!(matches!(
            convert_json_to_cbor(too_deep),
            Err(JsonCborError::MaxDepthExceeded)
        ));
    }
}
