//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Event decode: the indexer serves each template-emitted event (e.g. on `/transactions/events/stream`) as its CBOR
//! encoding in base64. [`decode_event`] turns that string back into the engine [`Event`].

use serde::Deserialize;
use tari_engine_types::{encoded::Encoded, events::Event};

use crate::types::error::OotleSdkError;

/// Decodes an event as the indexer serves it (a JSON string holding its base64 CBOR encoding), passed verbatim.
///
/// The [`Event`] serializes to the same JSON that `ootle-wasm-bor`'s `decodeEvent` returns.
///
/// Errors: a value that is not a base64 string, or whose bytes are not an `Event`, surfaces as `Parse`.
pub fn decode_event(event: &serde_json::Value) -> Result<Event, OotleSdkError> {
    Encoded::<Event>::deserialize(event)
        .map_err(|e| OotleSdkError::Parse(format!("event is not a base64 CBOR string: {e}")))?
        .decode()
        .map_err(|e| OotleSdkError::Parse(format!("undecodable event: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn served(event: &Event) -> serde_json::Value {
        serde_json::to_value(Encoded::encode(event).unwrap()).unwrap()
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
    fn a_non_string_is_rejected() {
        assert!(matches!(
            decode_event(&serde_json::json!({ "topic": "x" })),
            Err(OotleSdkError::Parse(_))
        ));
    }

    #[test]
    fn bytes_that_are_not_an_event_are_rejected() {
        let not_an_event = serde_json::to_value(Encoded::encode(&"hello".to_string()).unwrap()).unwrap();

        assert!(matches!(decode_event(&not_an_event), Err(OotleSdkError::Parse(_))));
    }
}
