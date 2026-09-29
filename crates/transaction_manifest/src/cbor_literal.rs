//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-clause

//! The `cbor!` and `metadata!` argument macros: a JSON-shaped value written directly as manifest
//! tokens and converted to a CBOR value tree.
//!
//! The value is read off the token trees `proc_macro2` has already lexed. A negative number is the
//! `-` punct followed by an integer literal, and an integer keeps its full CBOR range. Any other
//! argument macro may appear in value position, so a typed value such as `address!("resource_..")`
//! or `amount!(100)` is encoded as its own type:
//!
//! ```text
//! metadata!({
//!     "name": "My NFT",
//!     "resource": address!("resource_0123.."),
//!     "price": amount!(1000),
//!     "nested": { "index": -1, "tags": ["a", "b"] },
//! })
//! ```

use syn::{
    Ident,
    Lit,
    LitInt,
    LitStr,
    Macro,
    MacroDelimiter,
    Token,
    braced,
    bracketed,
    parse::{ParseStream, Parser},
    parse2,
    spanned::Spanned,
    token,
};
use tari_bor::{RawCbor, Value};
use tari_template_lib_types::{Metadata, constants::TARI_TOKEN};

use crate::{
    parser::{ManifestLiteral, OrVar, SpecialLiteral, handle_macro_argument},
    value::address_value,
};

/// A bound on how deeply one literal nests, so that parsing stays shallow. A nested `cbor!` or
/// `metadata!` starts its own count, and the engine rejects any metadata value nested past
/// [`tari_bor::MAX_DECODE_DEPTH`] in all.
const MAX_DEPTH: usize = tari_bor::MAX_DECODE_DEPTH;

/// The value a `cbor!` invocation writes.
///
/// A brace-delimited invocation, `cbor!{"a": 1}`, is the object itself; any other delimiter holds
/// one value, `cbor!({"a": 1})` or `cbor!([1, 2])`.
pub(crate) fn cbor_from_macro(mac: &Macro) -> syn::Result<Value> {
    match mac.delimiter {
        MacroDelimiter::Brace(_) => (|input: ParseStream| parse_object_body(input, 1)).parse2(mac.tokens.clone()),
        MacroDelimiter::Paren(_) | MacroDelimiter::Bracket(_) => {
            (|input: ParseStream| parse_value(input, 0)).parse2(mac.tokens.clone())
        },
    }
}

/// The metadata a `metadata!` invocation writes: an object whose keys are the metadata keys, each
/// value stored as its own CBOR encoding. An empty invocation is empty metadata, and a single
/// string literal is read as `key=value` pairs, each value a text string.
pub(crate) fn metadata_from_macro(mac: &Macro) -> syn::Result<Metadata> {
    if mac.tokens.is_empty() {
        return Ok(Metadata::new());
    }
    if !matches!(mac.delimiter, MacroDelimiter::Brace(_)) &&
        let Ok(lit) = parse2::<LitStr>(mac.tokens.clone())
    {
        return lit
            .value()
            .parse()
            .map_err(|e| syn::Error::new_spanned(&lit, format!("Failed to parse metadata key=value pairs: {e}")));
    }

    let Value::Map(entries) = cbor_from_macro(mac)? else {
        return Err(syn::Error::new_spanned(
            &mac.tokens,
            r#"metadata! takes an object, e.g. metadata!({"name": "My NFT"})"#,
        ));
    };
    let mut metadata = Metadata::new();
    for (key, value) in entries {
        let Value::Text(key) = key else {
            unreachable!("parse_object_body only produces text keys");
        };
        let value = RawCbor::from_value(&value)
            .map_err(|e| syn::Error::new_spanned(&mac.tokens, format!("Failed to encode metadata value: {e}")))?;
        metadata.insert_raw(key, value);
    }
    Ok(metadata)
}

fn parse_value(input: ParseStream, depth: usize) -> syn::Result<Value> {
    if depth > MAX_DEPTH {
        return Err(input.error(format!("value nested deeper than {MAX_DEPTH} levels")));
    }

    if input.peek(token::Brace) {
        let content;
        braced!(content in input);
        return parse_object_body(&content, depth + 1);
    }
    if input.peek(token::Bracket) {
        let content;
        bracketed!(content in input);
        let mut items = Vec::new();
        while !content.is_empty() {
            items.push(parse_value(&content, depth + 1)?);
            if content.is_empty() {
                break;
            }
            content.parse::<Token![,]>()?;
        }
        return Ok(Value::Array(items));
    }
    if input.peek(Token![-]) {
        input.parse::<Token![-]>()?;
        return match input.parse::<Lit>()? {
            Lit::Int(lit) => negative_int_value(&lit),
            lit => Err(syn::Error::new_spanned(lit, "expected an integer after `-`")),
        };
    }
    if input.peek(Ident) && input.peek2(Token![!]) {
        return nested_macro_value(input.parse()?);
    }
    if input.peek(Ident) {
        let ident = input.parse::<Ident>()?;
        return match ident.to_string().as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            "null" | "None" => Ok(Value::Null),
            "TARI" => tari_bor::to_value(&TARI_TOKEN).map_err(|e| syn::Error::new_spanned(&ident, e)),
            _ => Err(syn::Error::new_spanned(
                &ident,
                format!(
                    "`{ident}` cannot appear here: a workspace variable's value is only known when the transaction \
                     runs, so it cannot be written into a literal"
                ),
            )),
        };
    }
    lit_value(input.parse()?)
}

/// The entries of a `{ "key": value, .. }` object, without its braces. Keys are string literals
/// and each appears at most once.
fn parse_object_body(input: ParseStream, depth: usize) -> syn::Result<Value> {
    let mut entries: Vec<(Value, Value)> = Vec::new();
    while !input.is_empty() {
        let key = input
            .parse::<LitStr>()
            .map_err(|e| syn::Error::new(e.span(), "object keys must be string literals"))?;
        input.parse::<Token![:]>()?;
        let value = parse_value(input, depth)?;

        let key_value = Value::Text(key.value());
        if entries.iter().any(|(existing, _)| *existing == key_value) {
            return Err(syn::Error::new_spanned(
                &key,
                format!("key \"{}\" appears more than once", key.value()),
            ));
        }
        entries.push((key_value, value));

        if input.is_empty() {
            break;
        }
        input.parse::<Token![,]>()?;
    }
    Ok(Value::Map(entries))
}

fn lit_value(lit: Lit) -> syn::Result<Value> {
    match lit {
        Lit::Str(s) => Ok(Value::Text(s.value())),
        Lit::Int(i) => {
            let value = i.base10_parse::<i128>()?;
            check_int_suffix(&i, value)?;
            Value::integer(value).ok_or_else(|| syn::Error::new_spanned(&i, "integer out of CBOR range"))
        },
        Lit::Bool(b) => Ok(Value::Bool(b.value)),
        Lit::ByteStr(b) => Ok(Value::Bytes(b.value())),
        Lit::Char(c) => Ok(Value::Text(c.value().to_string())),
        lit @ Lit::Float(_) => Err(syn::Error::new_spanned(lit, "float literals are not supported")),
        lit => Err(syn::Error::new_spanned(lit, "unsupported literal")),
    }
}

pub(crate) fn negative_int_value(lit: &LitInt) -> syn::Result<Value> {
    let value = -lit.base10_parse::<i128>()?;
    check_int_suffix(lit, value)?;
    Value::integer(value).ok_or_else(|| syn::Error::new_spanned(lit, "integer out of CBOR range"))
}

/// A suffixed integer must fit its suffix's type. The suffix does not change the encoding — a CBOR
/// integer is the same whatever width it was written at — only what the author said it would fit.
fn check_int_suffix(lit: &LitInt, value: i128) -> syn::Result<()> {
    let fits = match lit.suffix() {
        "" | "i128" => true,
        "u8" => u8::try_from(value).is_ok(),
        "u16" => u16::try_from(value).is_ok(),
        "u32" => u32::try_from(value).is_ok(),
        "u64" => u64::try_from(value).is_ok(),
        "u128" => u128::try_from(value).is_ok(),
        "i8" => i8::try_from(value).is_ok(),
        "i16" => i16::try_from(value).is_ok(),
        "i32" => i32::try_from(value).is_ok(),
        "i64" => i64::try_from(value).is_ok(),
        suffix => {
            return Err(syn::Error::new_spanned(
                lit,
                format!("unsupported integer suffix `{suffix}`"),
            ));
        },
    };
    if fits {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            lit,
            format!("{value} does not fit a `{}`", lit.suffix()),
        ))
    }
}

/// The value of an argument macro written in value position, e.g. `address!("resource_..")`.
fn nested_macro_value(mac: Macro) -> syn::Result<Value> {
    let span = mac.span();
    let to_value_err = |e: tari_bor::BorError| syn::Error::new(span, e);
    let workspace_err = || {
        syn::Error::new(
            span,
            "a workspace variable's value is only known when the transaction runs, so it cannot be written into a \
             literal",
        )
    };

    match handle_macro_argument(mac)? {
        ManifestLiteral::Special(special) => match special {
            SpecialLiteral::Null => Ok(Value::Null),
            SpecialLiteral::Amount(amount) => tari_bor::to_value(&amount).map_err(to_value_err),
            SpecialLiteral::NonFungibleId(id) => tari_bor::to_value(&id).map_err(to_value_err),
            SpecialLiteral::Cbor(value) => Ok(value),
            SpecialLiteral::Metadata(metadata) => tari_bor::to_value(&metadata).map_err(to_value_err),
            SpecialLiteral::SubstateId(OrVar::Value(id)) => tari_bor::to_value(&id).map_err(to_value_err),
            SpecialLiteral::Address(OrVar::Value(id)) => address_value(&id).map_err(to_value_err),
            SpecialLiteral::SubstateId(OrVar::Var(_)) | SpecialLiteral::Address(OrVar::Var(_)) => Err(workspace_err()),
        },
        ManifestLiteral::Workspace(_) => Err(workspace_err()),
        ManifestLiteral::Blob(_) => Err(syn::Error::new(
            span,
            "blob! refers to a transaction blob and cannot be written into a literal",
        )),
        ManifestLiteral::Global(name) => Err(syn::Error::new(
            span,
            format!(
                "input variable \"{}\" cannot be written into a literal; pass it as its own argument or in a list",
                name.value()
            ),
        )),
        ManifestLiteral::Array(_) => Err(syn::Error::new(span, "a list cannot be written into a literal")),
        ManifestLiteral::Lit(lit) => lit_value(lit),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use proc_macro2::TokenStream;
    use tari_template_lib_types::{Amount, ResourceAddress};

    use super::*;

    fn mac(src: &str) -> Macro {
        parse2(TokenStream::from_str(src).unwrap()).unwrap()
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    #[test]
    fn nested_objects_and_arrays() {
        let value = cbor_from_macro(&mac(r#"cbor!({"a": 123, "b": {"c": "d"}, "e": [1, 2, 3]})"#)).unwrap();
        assert_eq!(
            value,
            Value::Map(vec![
                (text("a"), Value::Integer(123)),
                (text("b"), Value::Map(vec![(text("c"), text("d"))])),
                (
                    text("e"),
                    Value::Array(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)])
                ),
            ])
        );
    }

    #[test]
    fn a_brace_delimited_invocation_is_the_object() {
        assert_eq!(
            cbor_from_macro(&mac(r#"cbor!{"a": 1}"#)).unwrap(),
            cbor_from_macro(&mac(r#"cbor!({"a": 1})"#)).unwrap()
        );
    }

    #[test]
    fn negative_numbers() {
        assert_eq!(
            cbor_from_macro(&mac(r#"cbor!([-123, -9223372036854775808])"#)).unwrap(),
            Value::Array(vec![Value::Integer(-123), Value::Integer(i128::from(i64::MIN))])
        );
    }

    #[test]
    fn integers_keep_their_full_range() {
        assert_eq!(
            cbor_from_macro(&mac("cbor!(18446744073709551615)")).unwrap(),
            Value::Integer(i128::from(u64::MAX))
        );
        assert!(cbor_from_macro(&mac("cbor!(18446744073709551616)")).is_err());
    }

    #[test]
    fn a_suffixed_integer_must_fit_its_suffix() {
        assert_eq!(cbor_from_macro(&mac("cbor!(255u8)")).unwrap(), Value::Integer(255));
        assert!(cbor_from_macro(&mac("cbor!(256u8)")).is_err());
        assert!(cbor_from_macro(&mac("cbor!(-1u32)")).is_err());
    }

    #[test]
    fn keywords() {
        assert_eq!(
            cbor_from_macro(&mac("cbor!([true, false, null, None])")).unwrap(),
            Value::Array(vec![Value::Bool(true), Value::Bool(false), Value::Null, Value::Null])
        );
    }

    #[test]
    fn floats_are_rejected() {
        assert!(cbor_from_macro(&mac("cbor!(1.5)")).is_err());
        assert!(cbor_from_macro(&mac("cbor!(-1.5)")).is_err());
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let err = cbor_from_macro(&mac(r#"cbor!({"a": 1, "a": 2})"#)).unwrap_err();
        assert!(err.to_string().contains("more than once"), "{err}");
    }

    #[test]
    fn keys_must_be_strings() {
        assert!(cbor_from_macro(&mac("cbor!({1: 2})")).is_err());
    }

    #[test]
    fn a_workspace_variable_is_rejected() {
        let err = cbor_from_macro(&mac(r#"cbor!({"a": bucket})"#)).unwrap_err();
        assert!(err.to_string().contains("workspace variable"), "{err}");
        assert!(cbor_from_macro(&mac(r#"cbor!({"a": address!(bucket)})"#)).is_err());
    }

    #[test]
    fn typed_values_nest() {
        let resource =
            ResourceAddress::from_str("resource_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                .unwrap();
        let metadata = metadata_from_macro(&mac(r#"metadata!({
                "resource": address!("resource_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
                "price": amount!(1000),
                "native": TARI,
            })"#))
        .unwrap();

        assert_eq!(metadata.get_as::<ResourceAddress>("resource").unwrap(), Some(resource));
        assert_eq!(metadata.get_as::<Amount>("price").unwrap(), Some(Amount::from(1000u64)));
        assert_eq!(metadata.get_as::<ResourceAddress>("native").unwrap(), Some(TARI_TOKEN));
        // A typed address is its 32 bytes under a tag, not a hex string.
        assert!(metadata.get("resource").unwrap().as_bytes().len() < 40);
    }

    #[test]
    fn metadata_forms() {
        let expected = {
            let mut m = Metadata::new();
            m.insert("name", "Token");
            m
        };
        assert_eq!(
            metadata_from_macro(&mac(r#"metadata!({"name": "Token"})"#)).unwrap(),
            expected
        );
        assert_eq!(
            metadata_from_macro(&mac(r#"metadata!{"name": "Token"}"#)).unwrap(),
            expected
        );
        assert_eq!(
            metadata_from_macro(&mac(r#"metadata!("name=Token")"#)).unwrap(),
            expected
        );
        assert_eq!(metadata_from_macro(&mac("metadata!()")).unwrap(), Metadata::new());
        assert!(metadata_from_macro(&mac("metadata!([1, 2])")).is_err());
    }
}
