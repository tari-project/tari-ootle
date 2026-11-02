//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Full decoders for the engine values the indexer serves as their CBOR encoding in base64 (a JSON string): events
//! (e.g. on `/transactions/events/stream`), substate values, execute results, transaction receipts, resources and
//! UTXOs. Each takes the served JSON value verbatim and returns the engine type, whose serde JSON matches what
//! `ootle-wasm-bor` returns for the same input.

use serde::Deserialize;
use tari_engine_types::{
    Utxo,
    commit_result::ExecuteResult,
    encoded::Encoded,
    events::Event,
    resource::Resource,
    substate::SubstateValue,
    transaction_receipt::TransactionReceipt,
};

use crate::types::error::OotleSdkError;

/// Decodes a served `Event`. Errors: a value that is not a base64 CBOR `Event` surfaces as `Parse`.
pub fn decode_event(value: &serde_json::Value) -> Result<Event, OotleSdkError> {
    decode_served(value, "event")
}

/// Decodes a served `SubstateValue` in full (see [`decode_substate`](crate::decode_substate) for a summary view).
/// Errors: a value that is not a base64 CBOR `SubstateValue` surfaces as `Parse`.
pub fn decode_substate_value(value: &serde_json::Value) -> Result<SubstateValue, OotleSdkError> {
    decode_served(value, "substate value")
}

/// Decodes a served `ExecuteResult`. Errors: a value that is not a base64 CBOR `ExecuteResult` surfaces as `Parse`.
pub fn decode_execute_result(value: &serde_json::Value) -> Result<ExecuteResult, OotleSdkError> {
    decode_served(value, "execute result")
}

/// Decodes a served `TransactionReceipt`. Errors: a value that is not a base64 CBOR `TransactionReceipt` surfaces as
/// `Parse`.
pub fn decode_transaction_receipt(value: &serde_json::Value) -> Result<TransactionReceipt, OotleSdkError> {
    decode_served(value, "transaction receipt")
}

/// Decodes a served `Resource`. Errors: a value that is not a base64 CBOR `Resource` surfaces as `Parse`.
pub fn decode_resource(value: &serde_json::Value) -> Result<Resource, OotleSdkError> {
    decode_served(value, "resource")
}

/// Decodes a served `Utxo`. Errors: a value that is not a base64 CBOR `Utxo` surfaces as `Parse`.
pub fn decode_utxo(value: &serde_json::Value) -> Result<Utxo, OotleSdkError> {
    decode_served(value, "UTXO")
}

fn decode_served<T>(value: &serde_json::Value, what: &str) -> Result<T, OotleSdkError>
where T: for<'b> tari_bor::Decode<'b, ()> {
    Encoded::<T>::deserialize(value)
        .map_err(|e| OotleSdkError::Parse(format!("{what} is not a base64 CBOR string: {e}")))?
        .decode()
        .map_err(|e| OotleSdkError::Parse(format!("undecodable {what}: {e}")))
}

#[cfg(test)]
mod tests {
    use tari_engine_types::{resource_container::ResourceContainer, vault::Vault};
    use tari_template_lib_types::constants::TARI_TOKEN;

    use super::*;

    fn served<T: tari_bor::Encode<()>>(value: &T) -> serde_json::Value {
        serde_json::to_value(Encoded::encode(value).unwrap()).unwrap()
    }

    #[test]
    fn a_served_event_decodes_to_the_event() {
        let event = Event::new(
            None,
            Default::default(),
            "std.vault.deposit".to_string(),
            [("amount", "100".to_string())].into_iter().collect(),
        );

        assert_eq!(decode_event(&served(&event)).unwrap(), event);
    }

    #[test]
    fn a_served_substate_value_decodes_to_the_value() {
        let value = SubstateValue::Vault(Vault::new(ResourceContainer::public_fungible(TARI_TOKEN, 100u64)));

        let decoded = decode_substate_value(&served(&value)).unwrap();

        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            serde_json::to_value(value).unwrap()
        );
    }

    #[test]
    fn a_non_string_is_rejected() {
        assert!(matches!(
            decode_event(&serde_json::json!({ "topic": "x" })),
            Err(OotleSdkError::Parse(_))
        ));
    }

    #[test]
    fn bytes_of_another_type_are_rejected() {
        let value = SubstateValue::Vault(Vault::new(ResourceContainer::public_fungible(TARI_TOKEN, 100u64)));

        assert!(matches!(decode_event(&served(&value)), Err(OotleSdkError::Parse(_))));
        assert!(matches!(
            decode_execute_result(&served(&value)),
            Err(OotleSdkError::Parse(_))
        ));
    }
}
