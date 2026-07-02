//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The single sealed-transfer canonicalizer.
//!
//! [`decode_and_canonicalize_sealed_transfer`] is the one source of truth for turning sealed
//! BOR-encoded transaction bytes back into a deterministic-field JSON value: it BOR-decodes the
//! envelope, **verifies every signature** on the recovered transaction (a bad signature is a hard
//! error, not a falsy value), serializes the transaction to JSON, and recursively nulls the
//! **byte-unstable** proof/signature scalars.
//!
//! It is the single source of truth for the decode + verify + null-set logic, so semantic comparison
//! of a sealed transfer is implemented exactly once.
//!
//! The function is **pure** and RNG-free (decode + verify draw no randomness).
//!
//! ## The null set (the contract)
//!
//! It is exactly `["agg_range_proof", "balance_proof", "signature"]`: the aggregated bulletproof
//! (`agg_range_proof`), the ElGamal-viewable balance proof (`balance_proof`), and every Schnorr
//! signature *scalar* (the `signature` sub-object inside each `TransactionSignature` /
//! `TransactionSealSignature`). The signer **public keys survive** the nulling — they lock the
//! key-selection contract (which key seals / authorizes) — so only proof/signature scalars are
//! zeroed. **Do not change this set without updating every consumer that compares against it.**

use serde_json::Value;
use tari_ootle_transaction::TransactionEnvelope;

use crate::types::{error::OotleSdkError, network::Network};

/// The byte-unstable fields nulled by [`null_unstable_fields`]. Kept as a named constant so the
/// *one* place the set is defined is unmistakable.
pub const UNSTABLE_NULL_SET: [&str; 3] = ["agg_range_proof", "balance_proof", "signature"];

/// Decodes sealed BOR-encoded transaction bytes (lowercase hex), verifies every signature, and
/// returns the decoded transaction as a JSON value with the byte-unstable [`UNSTABLE_NULL_SET`]
/// recursively nulled.
///
/// Pure and RNG-free.
///
/// - `network` — the L1 network the transfer targets. The wire envelope is self-describing, so decode and signature
///   verification do not need it; it is accepted for API symmetry with the other stealth entry points (and so a future
///   check can reject a cross-network seal without an ABI change).
/// - `sealed_hex` — the lowercase-hex `EncodedPublicTransfer::encoded_transaction` (the `TransactionEnvelope` wire
///   form).
///
/// # Errors
/// - [`OotleSdkError::Parse`] if `sealed_hex` is not valid hex.
/// - [`OotleSdkError::Encoding`] if the bytes do not BOR-decode into a `TransactionEnvelope`.
/// - [`OotleSdkError::Validation`] if any signature on the decoded transaction fails to verify.
/// - [`OotleSdkError::Encoding`] if the decoded transaction fails to serialize to JSON (practically impossible).
pub fn decode_and_canonicalize_sealed_transfer(_network: Network, sealed_hex: &str) -> Result<Value, OotleSdkError> {
    let bytes =
        hex::decode(sealed_hex).map_err(|e| OotleSdkError::Parse(format!("sealed transfer: invalid hex: {e}")))?;

    let tx = TransactionEnvelope::from_raw(bytes.into_boxed_slice())
        .decode()
        .map_err(|e| OotleSdkError::Encoding(format!("sealed transfer: BOR decode failed: {e:?}")))?;

    if !tx.verify_all_signatures() {
        return Err(OotleSdkError::Validation(
            "sealed transfer: one or more signatures failed to verify".to_string(),
        ));
    }

    let mut value = serde_json::to_value(&tx)
        .map_err(|e| OotleSdkError::Encoding(format!("sealed transfer: tx serialize failed: {e}")))?;
    null_unstable_fields(&mut value);
    Ok(value)
}

/// Recursively replaces the byte-unstable proof/signature fields ([`UNSTABLE_NULL_SET`]) with JSON
/// null, in place. The **one** implementation of the null set.
pub fn null_unstable_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in UNSTABLE_NULL_SET {
                if let Some(slot) = map.get_mut(key) {
                    *slot = Value::Null;
                }
            }
            for (_, v) in map.iter_mut() {
                null_unstable_fields(v);
            }
        },
        Value::Array(items) => {
            for v in items.iter_mut() {
                null_unstable_fields(v);
            }
        },
        _ => {},
    }
}

#[cfg(test)]
mod tests {
    use tari_crypto::{ristretto::RistrettoSecretKey, tari_utilities::ByteArray};
    use tari_template_lib_types::{ComponentAddress, ObjectKey, ResourceAddress};

    use super::*;
    use crate::{
        build_and_encode_public_transfer,
        keys::PublicTransferKeys,
        public_transfer::EncodedPublicTransfer,
        types::{
            address::{ComponentAddressStr, ResourceAddressStr},
            bytes::{PublicKeyBytes, SecretKeyBytes},
            intent::{InputRef, PublicTransferIntent, TransferRecipient},
            numeric::BoundaryAmount,
        },
    };

    /// A canonical Ristretto scalar from a low byte seed (mirrors the public_transfer test helper).
    fn scalar_bytes(seed: u8) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0] = seed;
        let s = RistrettoSecretKey::from_canonical_bytes(&b).expect("low scalar is canonical");
        s.as_bytes().try_into().expect("32-byte scalar")
    }

    /// Seals a public transfer and returns it (a source of a real sealed transaction whose signatures
    /// verify, for the canonicalizer tests).
    fn sealed_public() -> EncodedPublicTransfer {
        let component = ComponentAddress::new(ObjectKey::from_array([0xaa; ObjectKey::LENGTH])).to_string();
        let resource = ResourceAddress::new(ObjectKey::from_array([0xbb; ObjectKey::LENGTH])).to_string();
        let intent = PublicTransferIntent {
            from_account: ComponentAddressStr::parse(component.clone()).unwrap(),
            recipient: TransferRecipient::PublicKey(PublicKeyBytes::from_array([5u8; 32])),
            resource_address: ResourceAddressStr::parse(resource).unwrap(),
            amount: BoundaryAmount::new(1000),
            fee: BoundaryAmount::new(2000),
            inputs: vec![InputRef::versioned(component, 0)],
            min_epoch: None,
            max_epoch: None,
            dry_run: false,
        };
        let keys = PublicTransferKeys::new(SecretKeyBytes::from_array(scalar_bytes(11)));
        build_and_encode_public_transfer(Network::Esmeralda, &intent, &keys).unwrap()
    }

    #[test]
    fn good_seal_canonicalizes_with_null_set_zeroed_and_pubkeys_present() {
        let out = sealed_public();
        let hex = out.encoded_transaction.to_hex();
        let value = decode_and_canonicalize_sealed_transfer(Network::Esmeralda, &hex).unwrap();
        // Every nulled key, wherever it occurs, must be JSON null.
        assert_all_unstable_nulled(&value);
        // A signer public key survives somewhere in the tree (the seal signer's public key).
        assert!(
            contains_nonnull_pubkey(&value),
            "signer public key must survive the nulling"
        );
    }

    #[test]
    fn deterministic_canonicalize_is_stable() {
        let out = sealed_public();
        let hex = out.encoded_transaction.to_hex();
        let a = decode_and_canonicalize_sealed_transfer(Network::Esmeralda, &hex).unwrap();
        let b = decode_and_canonicalize_sealed_transfer(Network::Esmeralda, &hex).unwrap();
        assert_eq!(a, b, "canonicalization is a pure function of its input");
    }

    #[test]
    fn invalid_hex_is_parse_error() {
        let e = decode_and_canonicalize_sealed_transfer(Network::Esmeralda, "zzzz").unwrap_err();
        assert_eq!(e.code(), "PARSE");
    }

    #[test]
    fn odd_length_hex_is_parse_error() {
        let e = decode_and_canonicalize_sealed_transfer(Network::Esmeralda, "abc").unwrap_err();
        assert_eq!(e.code(), "PARSE");
    }

    #[test]
    fn undecodable_bytes_are_encoding_error() {
        // Valid hex, but not a BOR-encoded TransactionEnvelope.
        let e = decode_and_canonicalize_sealed_transfer(Network::Esmeralda, "00010203").unwrap_err();
        assert_eq!(e.code(), "ENCODING");
    }

    #[test]
    fn tampered_signature_is_validation_error() {
        let out = sealed_public();
        let mut bytes = out.encoded_transaction.as_bytes().to_vec();
        // Flip the last byte: a real BOR-decodable transaction whose seal signature no longer
        // verifies. (The tail of the envelope is the seal signature scalar.)
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        let tampered_hex = hex::encode(&bytes);
        match decode_and_canonicalize_sealed_transfer(Network::Esmeralda, &tampered_hex) {
            // Either it still BOR-decodes and a signature now fails (VALIDATION), or the tail flip
            // broke the encoding (ENCODING). Both are hard errors, never a falsy ok.
            Err(e) => assert!(
                e.code() == "VALIDATION" || e.code() == "ENCODING",
                "a tampered seal must be a hard error, got code {}",
                e.code()
            ),
            Ok(_) => panic!("a tampered seal must not canonicalize successfully"),
        }
    }

    fn assert_all_unstable_nulled(value: &Value) {
        match value {
            Value::Object(map) => {
                for key in UNSTABLE_NULL_SET {
                    if let Some(slot) = map.get(key) {
                        assert!(slot.is_null(), "field `{key}` must be nulled, got {slot}");
                    }
                }
                for (_, v) in map {
                    assert_all_unstable_nulled(v);
                }
            },
            Value::Array(items) => items.iter().for_each(assert_all_unstable_nulled),
            _ => {},
        }
    }

    /// True if any object in the tree carries a non-null `public_key`-like field (the seal/auth
    /// signer public keys survive the nulling).
    fn contains_nonnull_pubkey(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    if (k.contains("public_key") || k == "public_nonce" || k.ends_with("_pk")) && !v.is_null() {
                        return true;
                    }
                    if contains_nonnull_pubkey(v) {
                        return true;
                    }
                }
                false
            },
            Value::Array(items) => items.iter().any(contains_nonnull_pubkey),
            _ => false,
        }
    }
}
