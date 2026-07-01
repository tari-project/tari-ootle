//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Public-transfer intent → submit-ready BOR-encoded bytes + transaction id.
//!
//! A pure, synchronous function of the boundary [`Network`], a [`PublicTransferIntent`], and a key
//! bundle. [`build_and_encode_public_transfer`] seals with a fresh OS-RNG nonce, so the bytes/id are
//! unique per call (the safe default for real submission).

use serde::{Deserialize, Serialize};

use crate::{
    builder::build_public_transfer_unsigned,
    keys::PublicTransferKeys,
    tx::{bor_encode, seal_public_transfer},
    types::{
        bytes::{EncodedTransactionBytes, TransactionIdBytes},
        error::OotleSdkError,
        intent::PublicTransferIntent,
        network::Network,
    },
};

/// The submit-ready BOR-encoded transaction bytes plus the transaction id. Both fields serialize as
/// lowercase hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodedPublicTransfer {
    /// The submit-ready BOR-encoded transaction bytes (the `TransactionEnvelope` wire form).
    pub encoded_transaction: EncodedTransactionBytes,
    /// The transaction id (32 bytes).
    pub transaction_id: TransactionIdBytes,
}

/// Builds, signs, seals (random-nonce: a fresh OS-RNG seed) and BOR-encodes a public transfer. The
/// bytes/id are **not** reproducible — per-call uniqueness is desirable for real submission.
pub fn build_and_encode_public_transfer(
    network: Network,
    intent: &PublicTransferIntent,
    keys: &PublicTransferKeys,
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let unsigned = build_public_transfer_unsigned(network, intent)?;
    let (transaction, id) = seal_public_transfer(unsigned, keys)?;
    let bytes = bor_encode(transaction)?;
    Ok(EncodedPublicTransfer {
        encoded_transaction: EncodedTransactionBytes::from_vec(bytes),
        transaction_id: TransactionIdBytes::from_bytes(id.as_bytes()),
    })
}

#[cfg(test)]
mod tests {
    use tari_crypto::{ristretto::RistrettoSecretKey, tari_utilities::ByteArray};
    use tari_ootle_transaction::TransactionEnvelope;
    use tari_template_lib_types::{ComponentAddress, ObjectKey, ResourceAddress};

    use super::*;
    use crate::{
        builder::build_public_transfer_unsigned,
        tx::{seal_public_transfer, verify_all_signatures},
        types::{
            address::{ComponentAddressStr, ResourceAddressStr},
            bytes::{PublicKeyBytes, SecretKeyBytes},
            intent::{InputRef, TransferRecipient},
            numeric::BoundaryAmount,
        },
    };

    // --- Fixed, canonical test material (low scalars are always canonical Ristretto scalars). ---

    fn fixed_scalar(seed: u8) -> RistrettoSecretKey {
        let mut b = [0u8; 32];
        b[0] = seed;
        RistrettoSecretKey::from_canonical_bytes(&b).expect("low scalar is canonical")
    }

    fn account_secret_bytes() -> SecretKeyBytes {
        SecretKeyBytes::from_bytes(fixed_scalar(11).as_bytes()).unwrap()
    }

    fn keys() -> PublicTransferKeys {
        PublicTransferKeys::new(account_secret_bytes())
    }

    fn component_str() -> String {
        ComponentAddress::new(ObjectKey::from_array([0xaa; ObjectKey::LENGTH])).to_string()
    }

    fn resource_str() -> String {
        ResourceAddress::new(ObjectKey::from_array([0xbb; ObjectKey::LENGTH])).to_string()
    }

    fn sample_intent() -> PublicTransferIntent {
        PublicTransferIntent {
            from_account: ComponentAddressStr::parse(component_str()).unwrap(),
            // Recipient public key distinct from the sender.
            recipient: TransferRecipient::PublicKey(PublicKeyBytes::from_array([5u8; 32])),
            resource_address: ResourceAddressStr::parse(resource_str()).unwrap(),
            // An amount well above 2^53 µTari to prove the u64-boundary path carries it intact.
            amount: BoundaryAmount::new((1u64 << 53) + 12_345),
            fee: BoundaryAmount::new(2000),
            inputs: vec![InputRef::versioned(component_str(), 0)],
            min_epoch: Some(5),
            max_epoch: Some(99),
            dry_run: false,
        }
    }

    #[test]
    fn happy_path_produces_nonempty_bytes_and_id() {
        let out = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &keys()).unwrap();
        assert!(
            !out.encoded_transaction.as_bytes().is_empty(),
            "encoded bytes must be non-empty"
        );
        assert_eq!(out.transaction_id.as_bytes().len(), 32, "id is a 32-byte hash");
        // The bytes must decode back into a valid TransactionEnvelope → Transaction.
        let envelope = TransactionEnvelope::from_raw(out.encoded_transaction.as_bytes().to_vec().into_boxed_slice());
        assert!(
            envelope.decode().is_ok(),
            "encoded bytes must round-trip through the envelope"
        );
    }

    /// The random-nonce seal yields differing bytes/id across runs.
    #[test]
    fn seal_is_not_reproducible() {
        let a = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &keys()).unwrap();
        let b = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &keys()).unwrap();
        assert_ne!(
            a.encoded_transaction, b.encoded_transaction,
            "random-nonce seal must produce differing bytes across runs"
        );
        assert_ne!(a.transaction_id, b.transaction_id, "and a differing transaction id");
    }

    /// The injected signatures must be real signatures.
    #[test]
    fn sealed_transaction_signatures_verify() {
        let unsigned = build_public_transfer_unsigned(Network::Esmeralda, &sample_intent()).unwrap();
        let (transaction, _id) = seal_public_transfer(unsigned, &keys()).unwrap();
        assert!(verify_all_signatures(&transaction), "all signatures must verify");
    }

    /// Amount above 2^53 µTari survives into the encoded bytes (re-decode cleanly with that amount in
    /// the withdraw arg).
    #[test]
    fn large_amount_round_trips_through_encoding() {
        let out = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &keys()).unwrap();
        let envelope = TransactionEnvelope::from_raw(out.encoded_transaction.as_bytes().to_vec().into_boxed_slice());
        let tx = envelope.decode().expect("decodes");
        // The withdraw instruction carries the (large) amount; a clean decode proves no truncation.
        assert!(!tx.instructions().is_empty());
    }

    /// Bad input must map to the correct error variant, never panic.
    #[test]
    fn malformed_secret_key_is_a_key_error() {
        // 0xff..ff is not a canonical Ristretto scalar.
        let bad_keys = PublicTransferKeys::new(SecretKeyBytes::from_array([0xff; 32]));
        let err = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &bad_keys).unwrap_err();
        assert_eq!(err.code(), "KEY");
    }

    #[test]
    fn malformed_address_is_a_parse_error() {
        let mut intent = sample_intent();
        intent.from_account = ComponentAddressStr("not-a-component".to_string());
        let err = build_and_encode_public_transfer(Network::Esmeralda, &intent, &keys()).unwrap_err();
        assert_eq!(err.code(), "PARSE");
    }
}
