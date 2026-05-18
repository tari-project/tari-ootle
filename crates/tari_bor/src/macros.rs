//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The [`cbor!`] macro for ergonomic construction of dynamic CBOR [`Value`] trees.
//!
//! Use this for tests, fixtures, and the rare runtime code that needs to build a `Value`
//! whose shape isn't known at compile time. Wire-format types should derive
//! [`minicbor::Encode`] / [`minicbor::Decode`] with `#[n(N)]` field tags instead.
//!
//! ```ignore
//! use tari_bor::{cbor, Value};
//!
//! let v = cbor!({
//!     "name" => "alice",
//!     "age" => 42u32,
//!     "tags" => ["a", "b", "c"],
//!     "nested" => { "x" => 1, "y" => 2 },
//!     "absent" => null,
//! });
//! ```
//!
//! Keys can be any expression that implements `Into<Value>` (typically a string literal or
//! integer). Values can be `null`, an array literal, a map literal, or any expression that
//! implements `Into<Value>` — including the result of another `cbor!()` call.
//!
//! [`Value`]: crate::Value

/// Construct a [`Value`](crate::Value) from a JSON-like literal.
///
/// See module-level docs for syntax.
#[macro_export]
macro_rules! cbor {
    (null) => { $crate::Value::Null };

    ({}) => { $crate::Value::Map($crate::__cbor_macro::vec_new()) };

    ([]) => { $crate::Value::Array($crate::__cbor_macro::vec_new()) };

    ({ $($tt:tt)+ }) => {{
        let mut __m: $crate::__cbor_macro::Vec<($crate::Value, $crate::Value)> =
            $crate::__cbor_macro::vec_new();
        $crate::__cbor_map_entries!(__m, $($tt)+);
        $crate::Value::Map(__m)
    }};

    ([ $($tt:tt)+ ]) => {{
        let mut __a: $crate::__cbor_macro::Vec<$crate::Value> = $crate::__cbor_macro::vec_new();
        $crate::__cbor_array_entries!(__a, $($tt)+);
        $crate::Value::Array(__a)
    }};

    ($v:expr) => { $crate::Value::from($v) };
}

/// Internal: parse `key => value, key => value, ...` map entries via tt-munching.
#[macro_export]
#[doc(hidden)]
macro_rules! __cbor_map_entries {
    // === Terminal arms (last entry, with optional trailing comma) ===
    ($m:ident, $k:tt => null $(,)?) => {
        $m.push(($crate::__cbor_macro::key($k), $crate::Value::Null));
    };
    ($m:ident, $k:tt => [ $($inner:tt)* ] $(,)?) => {
        $m.push(($crate::__cbor_macro::key($k), $crate::cbor!([ $($inner)* ])));
    };
    ($m:ident, $k:tt => { $($inner:tt)* } $(,)?) => {
        $m.push(($crate::__cbor_macro::key($k), $crate::cbor!({ $($inner)* })));
    };
    ($m:ident, $k:tt => $v:expr $(,)?) => {
        $m.push(($crate::__cbor_macro::key($k), $crate::Value::from($v)));
    };

    // === Continuation arms (entry followed by comma + more) ===
    ($m:ident, $k:tt => null, $($rest:tt)+) => {
        $m.push(($crate::__cbor_macro::key($k), $crate::Value::Null));
        $crate::__cbor_map_entries!($m, $($rest)+);
    };
    ($m:ident, $k:tt => [ $($inner:tt)* ], $($rest:tt)+) => {
        $m.push(($crate::__cbor_macro::key($k), $crate::cbor!([ $($inner)* ])));
        $crate::__cbor_map_entries!($m, $($rest)+);
    };
    ($m:ident, $k:tt => { $($inner:tt)* }, $($rest:tt)+) => {
        $m.push(($crate::__cbor_macro::key($k), $crate::cbor!({ $($inner)* })));
        $crate::__cbor_map_entries!($m, $($rest)+);
    };
    ($m:ident, $k:tt => $v:expr, $($rest:tt)+) => {
        $m.push(($crate::__cbor_macro::key($k), $crate::Value::from($v)));
        $crate::__cbor_map_entries!($m, $($rest)+);
    };
}

/// Internal: parse `value, value, ...` array entries via tt-munching.
#[macro_export]
#[doc(hidden)]
macro_rules! __cbor_array_entries {
    // Terminal arms
    ($a:ident, null $(,)?) => {
        $a.push($crate::Value::Null);
    };
    ($a:ident, [ $($inner:tt)* ] $(,)?) => {
        $a.push($crate::cbor!([ $($inner)* ]));
    };
    ($a:ident, { $($inner:tt)* } $(,)?) => {
        $a.push($crate::cbor!({ $($inner)* }));
    };
    ($a:ident, $v:expr $(,)?) => {
        $a.push($crate::Value::from($v));
    };

    // Continuation arms
    ($a:ident, null, $($rest:tt)+) => {
        $a.push($crate::Value::Null);
        $crate::__cbor_array_entries!($a, $($rest)+);
    };
    ($a:ident, [ $($inner:tt)* ], $($rest:tt)+) => {
        $a.push($crate::cbor!([ $($inner)* ]));
        $crate::__cbor_array_entries!($a, $($rest)+);
    };
    ($a:ident, { $($inner:tt)* }, $($rest:tt)+) => {
        $a.push($crate::cbor!({ $($inner)* }));
        $crate::__cbor_array_entries!($a, $($rest)+);
    };
    ($a:ident, $v:expr, $($rest:tt)+) => {
        $a.push($crate::Value::from($v));
        $crate::__cbor_array_entries!($a, $($rest)+);
    };
}

/// Internal helpers re-exported under a hidden module so the macro can reference them
/// without leaking implementation details onto `crate::`.
#[doc(hidden)]
pub mod __cbor_macro {
    #[cfg(not(feature = "std"))]
    pub use alloc::vec::Vec;
    #[cfg(feature = "std")]
    pub use std::vec::Vec;

    use crate::Value;

    pub fn vec_new<T>() -> Vec<T> {
        Vec::new()
    }

    /// Convert a key expression to a [`Value`]. Exists as a function (not just `Value::from`)
    /// so we can give nicer type errors and centralise the conversion if it ever needs to
    /// grow special cases.
    #[inline]
    pub fn key<K: Into<Value>>(k: K) -> Value {
        k.into()
    }
}

#[cfg(test)]
mod tests {
    use crate::Value;

    #[test]
    fn null_literal() {
        assert_eq!(cbor!(null), Value::Null);
    }

    #[test]
    fn primitive_literals() {
        assert_eq!(cbor!(true), Value::Bool(true));
        assert_eq!(cbor!(42u32), Value::Integer(42));
        assert_eq!(cbor!("hi"), Value::Text("hi".into()));
    }

    #[test]
    fn empty_collections() {
        assert_eq!(cbor!({}), Value::Map(vec![]));
        assert_eq!(cbor!([]), Value::Array(vec![]));
    }

    #[test]
    fn array_of_mixed_values() {
        let v = cbor!([1u32, "two", true, null]);
        assert_eq!(
            v,
            Value::Array(vec![
                Value::Integer(1),
                Value::Text("two".into()),
                Value::Bool(true),
                Value::Null
            ])
        );
    }

    #[test]
    fn flat_map() {
        let v = cbor!({
            "name" => "alice",
            "age" => 42u32,
            "active" => true,
        });
        assert_eq!(
            v,
            Value::Map(vec![
                (Value::Text("name".into()), Value::Text("alice".into())),
                (Value::Text("age".into()), Value::Integer(42)),
                (Value::Text("active".into()), Value::Bool(true)),
            ])
        );
    }

    #[test]
    fn nested_map_in_map() {
        let v = cbor!({
            "outer" => { "inner" => 1u32 },
        });
        assert_eq!(
            v,
            Value::Map(vec![(
                Value::Text("outer".into()),
                Value::Map(vec![(Value::Text("inner".into()), Value::Integer(1))]),
            )])
        );
    }

    #[test]
    fn nested_array_in_map() {
        let v = cbor!({
            "items" => [1u32, 2u32, 3u32],
        });
        assert_eq!(
            v,
            Value::Map(vec![(
                Value::Text("items".into()),
                Value::Array(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]),
            )])
        );
    }

    #[test]
    fn nested_array_in_array() {
        let v = cbor!([[1u32, 2u32], [3u32, 4u32]]);
        assert_eq!(
            v,
            Value::Array(vec![
                Value::Array(vec![Value::Integer(1), Value::Integer(2)]),
                Value::Array(vec![Value::Integer(3), Value::Integer(4)]),
            ])
        );
    }

    #[test]
    fn expression_values() {
        let x = 99u32;
        let s = String::from("dynamic");
        let v = cbor!({
            "x" => x,
            "s" => s.clone(),
        });
        assert_eq!(
            v,
            Value::Map(vec![
                (Value::Text("x".into()), Value::Integer(99)),
                (Value::Text("s".into()), Value::Text("dynamic".into())),
            ])
        );
    }

    #[test]
    fn integer_keys() {
        let v = cbor!({ 0u32 => "zero", 1u32 => "one" });
        assert_eq!(
            v,
            Value::Map(vec![
                (Value::Integer(0), Value::Text("zero".into())),
                (Value::Integer(1), Value::Text("one".into())),
            ])
        );
    }

    #[test]
    fn null_value_in_map() {
        let v = cbor!({ "missing" => null });
        assert_eq!(v, Value::Map(vec![(Value::Text("missing".into()), Value::Null)]));
    }

    #[test]
    fn trailing_comma() {
        let _ = cbor!([1u32, 2u32,]);
        let _ = cbor!({ "k" => "v", });
    }

    #[test]
    fn deeply_nested() {
        let v = cbor!({
            "a" => {
                "b" => {
                    "c" => [1u32, 2u32, [3u32, 4u32]],
                },
            },
        });
        if let Value::Map(outer) = &v {
            assert_eq!(outer.len(), 1);
        } else {
            panic!("expected map");
        }
    }

    #[test]
    fn round_trip_through_minicbor() {
        let v = cbor!({
            "name" => "alice",
            "tags" => ["a", "b"],
            "nested" => { "n" => 42u32 },
        });
        let bytes = minicbor::to_vec(&v).unwrap();
        let decoded: Value = minicbor::decode(&bytes).unwrap();
        assert_eq!(decoded, v);
    }
}
