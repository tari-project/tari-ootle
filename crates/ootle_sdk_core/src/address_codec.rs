//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Address parse/format codec for the two Ootle address kinds.
//!
//! 1. **Engine substate addresses** — `component_<hex>` / `resource_<hex>` (the canonical strings
//!    `tari_template_lib_types` produces). Not bech32m. Parsing wraps the `*Str::parse` validators in
//!    [`crate::types::address`]; formatting is their `Display`.
//! 2. **Identity / payment address** — the `otl_…` bech32m recipient handle carrying `{network, account_key,
//!    view_only_key, pay_ref}`. The bech32m codec lives in the [`tari_ootle_address`] crate; this module calls it
//!    (`OotleAddress::decode_bech32` / `encode_bech32_to_fmt`).
//!
//! [`parse_address`] dispatches on the string prefix and returns a kind-tagged [`ParsedAddress`].
//! [`format_identity_address`] constructs an [`OotleAddress`] and bech32m-encodes it under the
//! network-qualified HRP the encoder derives from `network` (MainNet `otl_`, LocalNet `otl_loc_`,
//! Esmeralda `otl_esm_`, …).
//!
//! Error mapping: an unknown prefix, a malformed bech32m string (bad checksum / HRP / length), a
//! substate-id parse failure, bad/odd/uppercase `pay_ref` hex, or an oversize `pay_ref` (> 64 bytes)
//! all map to [`OotleSdkError::Parse`].

use serde::{Deserialize, Serialize};
use tari_ootle_address::{OotleAddress, PayRef};

use crate::types::{
    address::{ComponentAddressStr, ResourceAddressStr},
    bytes::PublicKeyBytes,
    error::OotleSdkError,
    network::Network,
};

/// The `otl` stem shared by every network-qualified identity HRP (MainNet `otl_`, LocalNet
/// `otl_loc_`, …). A parse candidate that starts with this stem is routed to the bech32m identity
/// decoder, which validates the *full* HRP. (Substate ids never collide: they start with
/// `component_` / `resource_` / … and the engine canonical strings are not bech32m.)
const IDENTITY_HRP_STEM: &str = "otl";

/// A parsed address, tagged by kind so a host never confuses an engine substate id with an `otl_…`
/// recipient identity (mis-routing a recipient identity as a component address is a silent
/// value error).
///
/// Substate kinds carry only their canonical `<prefix>_<hex>` string; the identity kind carries its
/// fully-decoded fields plus the re-encoded canonical bech32m string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ParsedAddress {
    /// An engine `component_<hex>` substate address.
    Component {
        /// The canonical `component_<hex>` string.
        canonical: String,
    },
    /// An engine `resource_<hex>` substate address.
    Resource {
        /// The canonical `resource_<hex>` string.
        canonical: String,
    },
    /// An `otl_…` bech32m identity / payment address.
    Identity {
        /// The network the HRP encodes (`otl_` MainNet, `otl_loc_` LocalNet, …).
        network: Network,
        /// The account (spend) public key, lowercase hex.
        account_key: PublicKeyBytes,
        /// The view-only public key, lowercase hex.
        view_only_key: PublicKeyBytes,
        /// The optional payment reference, lowercase hex (`None` if absent).
        #[serde(skip_serializing_if = "Option::is_none")]
        pay_ref: Option<String>,
        /// The canonical bech32m string this parsed from (re-encoded; round-trips byte-for-byte).
        bech32m: String,
    },
}

/// Parses an address string into a kind-tagged [`ParsedAddress`], dispatching on its prefix.
///
/// - `component_<hex>` / `resource_<hex>` → the existing engine substate validators ([`ComponentAddressStr::parse`] /
///   [`ResourceAddressStr::parse`]).
/// - an `otl`-stem string → the bech32m identity decoder ([`OotleAddress::decode_bech32`], which validates the full
///   network-qualified HRP and the checksum).
///
/// Any unknown prefix, or a malformed substate id / bech32m string, maps to [`OotleSdkError::Parse`].
/// The identity fields are read back by name so the two keys can never be swapped.
pub fn parse_address(s: &str) -> Result<ParsedAddress, OotleSdkError> {
    let trimmed = s.trim();
    if trimmed.starts_with("component_") {
        let canonical = ComponentAddressStr::parse(trimmed)?.0;
        Ok(ParsedAddress::Component { canonical })
    } else if trimmed.starts_with("resource_") {
        let canonical = ResourceAddressStr::parse(trimmed)?.0;
        Ok(ParsedAddress::Resource { canonical })
    } else if trimmed.starts_with(IDENTITY_HRP_STEM) {
        let addr = OotleAddress::decode_bech32(trimmed)
            .map_err(|e| OotleSdkError::Parse(format!("invalid identity address '{trimmed}': {e}")))?;
        Ok(identity_from_ootle_address(&addr))
    } else {
        Err(OotleSdkError::Parse(format!(
            "unrecognised address prefix in '{trimmed}': expected 'component_', 'resource_', or an 'otl' identity HRP"
        )))
    }
}

/// Formats an `otl_…` identity / payment address (bech32m) from its parts.
///
/// Constructs the [`OotleAddress`] by name and bech32m-encodes it via the crate's own codec
/// ([`OotleAddress::encode_bech32_to_fmt`]); the HRP is derived from `network`, so the encoded prefix
/// is network-qualified.
///
/// `pay_ref` is an optional lowercase-hex string. Bad/odd/uppercase hex, or a decoded length over the
/// 64-byte cap, map to [`OotleSdkError::Parse`].
pub fn format_identity_address(
    network: Network,
    account_key: &PublicKeyBytes,
    view_only_key: &PublicKeyBytes,
    pay_ref: Option<&str>,
) -> Result<String, OotleSdkError> {
    // `OotleAddress::new` takes (network, view_only_key, account_key) in that order — pass the keys
    // by name to avoid a silent swap.
    let mut addr = OotleAddress::new(network.into(), view_only_key.to_internal(), account_key.to_internal());

    if let Some(pr_hex) = pay_ref {
        addr = addr.with_pay_ref(pay_ref_from_hex(pr_hex)?);
    }

    let mut out = String::new();
    addr.encode_bech32_to_fmt(&mut out)
        .map_err(|e| OotleSdkError::Parse(format!("failed to bech32m-encode identity address: {e}")))?;
    Ok(out)
}

/// Maps a decoded [`OotleAddress`] into the [`ParsedAddress::Identity`] variant, reading every field
/// by name and re-encoding the canonical bech32m string (so the parsed record carries the exact form
/// it round-trips to).
fn identity_from_ootle_address(addr: &OotleAddress) -> ParsedAddress {
    ParsedAddress::Identity {
        network: Network::from(addr.network()),
        account_key: PublicKeyBytes::from_internal(addr.account_public_key()),
        view_only_key: PublicKeyBytes::from_internal(addr.view_only_key()),
        pay_ref: addr.pay_ref().map(|pr| hex::encode(pr.as_bytes())),
        bech32m: addr.to_bech32_string(),
    }
}

/// Decodes a lowercase-hex `pay_ref` into a [`PayRef`], enforcing the lowercase-hex contract and the
/// 64-byte cap. Any violation is [`OotleSdkError::Parse`].
fn pay_ref_from_hex(pr_hex: &str) -> Result<PayRef, OotleSdkError> {
    if pr_hex.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(OotleSdkError::Parse("pay_ref must be lowercase hex".to_string()));
    }
    let bytes = hex::decode(pr_hex).map_err(|e| OotleSdkError::Parse(format!("invalid pay_ref hex: {e}")))?;
    // `new_checked` rejects both empty and > MAX_LEN: a present-but-empty pay_ref is meaningless, so
    // empty hex is a parse error.
    PayRef::new_checked(bytes).ok_or_else(|| {
        OotleSdkError::Parse(format!(
            "pay_ref must be 1..={} bytes (got an empty or oversize value)",
            PayRef::MAX_LEN
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic 32-byte public-key-shaped value (not necessarily on-curve — the codec only
    /// moves bytes, it does not validate curve points).
    fn pk(seed: u8) -> PublicKeyBytes {
        PublicKeyBytes::from_array([seed; 32])
    }

    fn account() -> PublicKeyBytes {
        pk(0xA1)
    }
    fn view() -> PublicKeyBytes {
        pk(0xB2)
    }

    #[test]
    fn identity_round_trips_format_then_parse() {
        for network in [
            Network::MainNet,
            Network::LocalNet,
            Network::Esmeralda,
            Network::Igor,
            Network::NextNet,
            Network::StageNet,
        ] {
            let s = format_identity_address(network, &account(), &view(), None).unwrap();
            let parsed = parse_address(&s).unwrap();
            match parsed {
                ParsedAddress::Identity {
                    network: n,
                    account_key,
                    view_only_key,
                    pay_ref,
                    bech32m,
                } => {
                    assert_eq!(n, network, "the network byte must survive the round-trip");
                    // The keys must come back unswapped.
                    assert_eq!(account_key, account(), "account_key must round-trip (not the view key)");
                    assert_eq!(
                        view_only_key,
                        view(),
                        "view_only_key must round-trip (not the account key)"
                    );
                    assert_eq!(pay_ref, None);
                    assert_eq!(bech32m, s, "the re-encoded bech32m equals the parsed input");
                },
                other => panic!("expected Identity, got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_then_format_is_identity() {
        let s = format_identity_address(Network::Esmeralda, &account(), &view(), None).unwrap();
        let ParsedAddress::Identity {
            network,
            account_key,
            view_only_key,
            ..
        } = parse_address(&s).unwrap()
        else {
            panic!("expected Identity");
        };
        let reformatted = format_identity_address(network, &account_key, &view_only_key, None).unwrap();
        assert_eq!(reformatted, s, "format ∘ parse == id");
    }

    #[test]
    fn identity_with_pay_ref_round_trips() {
        let pr_hex = "deadbeef";
        let s = format_identity_address(Network::LocalNet, &account(), &view(), Some(pr_hex)).unwrap();
        let ParsedAddress::Identity { pay_ref, .. } = parse_address(&s).unwrap() else {
            panic!("expected Identity");
        };
        assert_eq!(
            pay_ref.as_deref(),
            Some(pr_hex),
            "pay_ref must round-trip lowercase hex"
        );
    }

    #[test]
    fn keys_are_not_swapped_distinct() {
        // Distinct keys: a swap would be observable here even without the explicit eq checks above.
        let s = format_identity_address(Network::Esmeralda, &account(), &view(), None).unwrap();
        let ParsedAddress::Identity {
            account_key,
            view_only_key,
            ..
        } = parse_address(&s).unwrap()
        else {
            panic!("expected Identity");
        };
        assert_ne!(account_key, view_only_key);
        assert_eq!(account_key, account());
        assert_eq!(view_only_key, view());
    }

    #[test]
    fn pay_ref_at_cap_is_accepted_and_over_cap_rejected() {
        // Exactly 64 bytes (the cap) is accepted.
        let at_cap = "aa".repeat(PayRef::MAX_LEN);
        assert!(format_identity_address(Network::Esmeralda, &account(), &view(), Some(&at_cap)).is_ok());

        // 65 bytes is rejected with PARSE.
        let over_cap = "aa".repeat(PayRef::MAX_LEN + 1);
        let err = format_identity_address(Network::Esmeralda, &account(), &view(), Some(&over_cap)).unwrap_err();
        assert_eq!(err.code(), "PARSE");
    }

    #[test]
    fn pay_ref_bad_hex_is_parse() {
        // Uppercase hex.
        let err = format_identity_address(Network::Esmeralda, &account(), &view(), Some("DEADBEEF")).unwrap_err();
        assert_eq!(err.code(), "PARSE");
        // Odd-length hex.
        let err = format_identity_address(Network::Esmeralda, &account(), &view(), Some("abc")).unwrap_err();
        assert_eq!(err.code(), "PARSE");
        // Empty pay_ref (use None for "no pay_ref").
        let err = format_identity_address(Network::Esmeralda, &account(), &view(), Some("")).unwrap_err();
        assert_eq!(err.code(), "PARSE");
    }

    #[test]
    fn parse_substate_component_and_resource() {
        use std::str::FromStr;

        use tari_template_lib_types::{ComponentAddress, ObjectKey, ResourceAddress};

        let component = ComponentAddress::new(ObjectKey::from_array([0x22; ObjectKey::LENGTH])).to_string();
        let resource = ResourceAddress::new(ObjectKey::from_array([0x11; ObjectKey::LENGTH])).to_string();

        match parse_address(&component).unwrap() {
            ParsedAddress::Component { canonical } => {
                assert_eq!(canonical, component);
                // The canonical string round-trips back to the internal type.
                assert!(ComponentAddress::from_str(&canonical).is_ok());
            },
            other => panic!("expected Component, got {other:?}"),
        }
        match parse_address(&resource).unwrap() {
            ParsedAddress::Resource { canonical } => assert_eq!(canonical, resource),
            other => panic!("expected Resource, got {other:?}"),
        }
    }

    #[test]
    fn unknown_prefix_is_parse() {
        let err = parse_address("nope_1234").unwrap_err();
        assert_eq!(err.code(), "PARSE");
        let err = parse_address("").unwrap_err();
        assert_eq!(err.code(), "PARSE");
    }

    #[test]
    fn bad_bech32m_checksum_is_parse() {
        // A well-formed identity address with the last data char corrupted ⇒ checksum failure.
        let mut s = format_identity_address(Network::Esmeralda, &account(), &view(), None).unwrap();
        let last = s.pop().unwrap();
        // Flip the final character to a different valid bech32 char to break the checksum.
        let replacement = if last == 'q' { 'p' } else { 'q' };
        s.push(replacement);
        let err = parse_address(&s).unwrap_err();
        assert_eq!(err.code(), "PARSE");
    }

    #[test]
    fn malformed_substate_hex_is_parse() {
        let err = parse_address("component_not_hex").unwrap_err();
        assert_eq!(err.code(), "PARSE");
        let err = parse_address("resource_zzzz").unwrap_err();
        assert_eq!(err.code(), "PARSE");
    }
}
