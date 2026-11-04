//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The flat `extern "C"` full-decode surface: each fn takes an engine value as the indexer serves it (a JSON string
//! holding its base64 CBOR encoding), passed verbatim, and returns the value's JSON in `data_json` — the same JSON
//! `ootle-wasm-bor` returns for that input.
//!
//! - [`ootle_decode_event`] — an `Event`, e.g. the `event` field of a `/transactions/events/stream` message.
//! - [`ootle_decode_substate_value`] — a `SubstateValue` in full
//!   ([`ootle_decode_substate`](crate::ootle_decode_substate) gives a kind-tagged summary).
//! - [`ootle_decode_execute_result`] — an `ExecuteResult`, e.g. a dry run's `result`.
//! - [`ootle_decode_transaction_receipt`] — a `TransactionReceipt`.
//! - [`ootle_decode_resource`] — a `Resource`.
//! - [`ootle_decode_utxo`] — a `Utxo`.
//!
//! All are stateless and return no handle. A value that is not the named type's base64 CBOR encoding yields
//! `"PARSE"`; a null arg yields `"INVALID"`. Engine amounts keep their engine serde form.

use std::os::raw::c_char;

use ootle_sdk_core::{
    decode_event,
    decode_execute_result,
    decode_resource,
    decode_substate_value,
    decode_transaction_receipt,
    decode_utxo,
    types::error::OotleSdkError,
};
use serde::Serialize;

use crate::c_abi::{OotleResult, flatten, guarded, output_json, parse_json, required_str};

/// Reads `ptr` as the served JSON value, decodes it with `decode`, and wraps the decoded value's JSON in the envelope.
///
/// # Safety
/// `ptr` must be NULL or a valid NUL-terminated UTF-8 C string.
unsafe fn decode_to_envelope<T: Serialize>(
    ptr: *const c_char,
    arg: &str,
    what: &str,
    decode: fn(&serde_json::Value) -> Result<T, OotleSdkError>,
) -> OotleResult {
    guarded(|| {
        flatten((|| {
            let json = unsafe { required_str(ptr, arg) }?;
            let value: serde_json::Value = parse_json(json, what)?;
            match decode(&value) {
                Ok(decoded) => Ok(OotleResult::ok_json(&output_json(&decoded, what)?)),
                Err(e) => Ok(OotleResult::from_core_err(&e)),
            }
        })())
    })
}

/// Decodes a served `Event`.
///
/// `event_json` is an `Event` as the indexer serves it (e.g. the `event` field of a `/transactions/events/stream`
/// message): a JSON string holding its base64 CBOR encoding, passed verbatim with its quotes (`"<base64>"`). Bare
/// base64 yields `"PARSE"`.
///
/// On success `data_json` is `{ "substate_id": "<id>" | null, "template_address": "<hex>",
/// "topic": "<topic>", "payload": { … } }`.
///
/// # Safety
/// `event_json` must be a valid NUL-terminated UTF-8 C string. The returned envelope must be freed with
/// [`ootle_result_free`](crate::ootle_result_free). It never carries a handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ootle_decode_event(event_json: *const c_char) -> OotleResult {
    unsafe { decode_to_envelope(event_json, "event_json", "event", decode_event) }
}

/// Decodes a served `SubstateValue` in full.
///
/// `substate_value_json` is a `SubstateValue` as the indexer serves it: a JSON string holding its base64 CBOR encoding,
/// passed verbatim with its quotes (`"<base64>"`). Bare base64 yields `"PARSE"`.
///
/// On success `data_json` is the value's serde JSON, keyed by variant
/// (`{ "Component": { … } }`, `{ "Vault": { … } }`, …).
///
/// # Safety
/// `substate_value_json` must be a valid NUL-terminated UTF-8 C string. The returned envelope must be freed with
/// [`ootle_result_free`](crate::ootle_result_free). It never carries a handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ootle_decode_substate_value(substate_value_json: *const c_char) -> OotleResult {
    unsafe {
        decode_to_envelope(
            substate_value_json,
            "substate_value_json",
            "substate value",
            decode_substate_value,
        )
    }
}

/// Decodes a served `ExecuteResult`.
///
/// `execute_result_json` is an `ExecuteResult` as the indexer serves it (e.g. a dry run's `result`): a JSON string
/// holding its base64 CBOR encoding, passed verbatim with its quotes (`"<base64>"`). Bare base64 yields `"PARSE"`.
///
/// On success `data_json` is its serde JSON.
///
/// # Safety
/// `execute_result_json` must be a valid NUL-terminated UTF-8 C string. The returned envelope must be freed with
/// [`ootle_result_free`](crate::ootle_result_free). It never carries a handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ootle_decode_execute_result(execute_result_json: *const c_char) -> OotleResult {
    unsafe {
        decode_to_envelope(
            execute_result_json,
            "execute_result_json",
            "execute result",
            decode_execute_result,
        )
    }
}

/// Decodes a served `TransactionReceipt`.
///
/// `receipt_json` is a `TransactionReceipt` as the indexer serves it: a JSON string holding its base64 CBOR encoding,
/// passed verbatim with its quotes (`"<base64>"`). Bare base64 yields `"PARSE"`.
///
/// On success `data_json` is its serde JSON.
///
/// # Safety
/// `receipt_json` must be a valid NUL-terminated UTF-8 C string. The returned envelope must be freed with
/// [`ootle_result_free`](crate::ootle_result_free). It never carries a handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ootle_decode_transaction_receipt(receipt_json: *const c_char) -> OotleResult {
    unsafe {
        decode_to_envelope(
            receipt_json,
            "receipt_json",
            "transaction receipt",
            decode_transaction_receipt,
        )
    }
}

/// Decodes a served `Resource`.
///
/// `resource_json` is a `Resource` as the indexer serves it: a JSON string holding its base64 CBOR encoding, passed
/// verbatim with its quotes (`"<base64>"`). Bare base64 yields `"PARSE"`.
///
/// On success `data_json` is its serde JSON.
///
/// # Safety
/// `resource_json` must be a valid NUL-terminated UTF-8 C string. The returned envelope must be freed with
/// [`ootle_result_free`](crate::ootle_result_free). It never carries a handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ootle_decode_resource(resource_json: *const c_char) -> OotleResult {
    unsafe { decode_to_envelope(resource_json, "resource_json", "resource", decode_resource) }
}

/// Decodes a served `Utxo`.
///
/// `utxo_json` is a `Utxo` as the indexer serves it: a JSON string holding its base64 CBOR encoding, passed verbatim
/// with its quotes (`"<base64>"`). Bare base64 yields `"PARSE"`.
///
/// On success `data_json` is its serde JSON.
///
/// # Safety
/// `utxo_json` must be a valid NUL-terminated UTF-8 C string. The returned envelope must be freed with
/// [`ootle_result_free`](crate::ootle_result_free). It never carries a handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ootle_decode_utxo(utxo_json: *const c_char) -> OotleResult {
    unsafe { decode_to_envelope(utxo_json, "utxo_json", "UTXO", decode_utxo) }
}
