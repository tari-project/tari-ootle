//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The single structured error envelope for `ootle-sdk-core`.
//!
//! Every variant carries a **stable machine code** via [`OotleSdkError::code`]. Host SDKs branch on
//! that code, so the codes are part of the public contract: never rename an existing code. The crate
//! stays generator-agnostic — facades map this enum to Kotlin exceptions / a Go result struct.

use thiserror::Error;

/// The one error type crossing the `ootle-sdk-core` boundary.
///
/// Each variant maps 1:1 to a stable [`code`](OotleSdkError::code) that host SDKs match on.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OotleSdkError {
    /// BOR/CBOR (or any byte-level) encoding/decoding failed.
    #[error("encoding error: {0}")]
    Encoding(String),
    /// A key, signature, or nonce secret was malformed or could not be derived.
    #[error("key error: {0}")]
    Key(String),
    /// A string/structured value failed to parse into an internal type (address, substate id, …).
    #[error("parse error: {0}")]
    Parse(String),
    /// A semantically-valid-looking value failed a domain rule (e.g. an amount exceeding the
    /// representable range, an empty input set, …).
    #[error("validation error: {0}")]
    Validation(String),
    /// A value is structurally invalid for the requested operation.
    #[error("invalid: {0}")]
    Invalid(String),
    /// Reserved for input resolution (want-list / fetched-substate resolution).
    #[error("resolution error: {0}")]
    Resolution(String),
    /// A confidential (stealth) transfer operation failed (entropy/proof/decrypt/spec error).
    #[error("stealth error: {0}")]
    Stealth(String),
}

impl OotleSdkError {
    /// Returns the **stable** machine code for this error.
    ///
    /// These strings are part of the public contract — host SDKs branch on them. Do **not** rename
    /// an existing code; only add new ones alongside new variants.
    pub fn code(&self) -> &'static str {
        match self {
            OotleSdkError::Encoding(_) => "ENCODING",
            OotleSdkError::Key(_) => "KEY",
            OotleSdkError::Parse(_) => "PARSE",
            OotleSdkError::Validation(_) => "VALIDATION",
            OotleSdkError::Invalid(_) => "INVALID",
            OotleSdkError::Resolution(_) => "RESOLUTION",
            OotleSdkError::Stealth(_) => "STEALTH",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_has_its_stable_code() {
        assert_eq!(OotleSdkError::Encoding("x".into()).code(), "ENCODING");
        assert_eq!(OotleSdkError::Key("x".into()).code(), "KEY");
        assert_eq!(OotleSdkError::Parse("x".into()).code(), "PARSE");
        assert_eq!(OotleSdkError::Validation("x".into()).code(), "VALIDATION");
        assert_eq!(OotleSdkError::Invalid("x".into()).code(), "INVALID");
        assert_eq!(OotleSdkError::Resolution("x".into()).code(), "RESOLUTION");
        assert_eq!(OotleSdkError::Stealth("x".into()).code(), "STEALTH");
    }

    #[test]
    fn stealth_display_includes_message() {
        let e = OotleSdkError::Stealth("bad entropy".into());
        assert!(e.to_string().contains("bad entropy"));
    }

    #[test]
    fn display_includes_message() {
        let e = OotleSdkError::Parse("bad address".into());
        assert!(e.to_string().contains("bad address"));
    }
}
