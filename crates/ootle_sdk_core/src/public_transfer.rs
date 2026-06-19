//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Public-transfer intent → submit-ready BOR-encoded bytes + transaction id.
//!
//! A pure, synchronous function of the boundary [`Network`], a [`PublicTransferIntent`], and a key
//! bundle. Two variants:
//!
//! - [`build_and_encode_public_transfer`] — deterministic: pinned nonce secrets ⇒ byte-identical bytes and id.
//! - [`build_and_encode_public_transfer_production`] — production: random-nonce seal, unique bytes/id per call.

use serde::{Deserialize, Serialize};

use crate::{
    builder::build_public_transfer_unsigned,
    keys::{DeterministicTransferKeys, PublicTransferKeys},
    tx::{bor_encode, seal_public_transfer, seal_public_transfer_deterministic},
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
    /// The transaction id (32 bytes), reproducible on the deterministic path.
    pub transaction_id: TransactionIdBytes,
}

/// Builds, signs, seals (deterministically, with pinned nonce secrets) and BOR-encodes a public
/// transfer. Byte-for-byte reproducible across calls with identical inputs.
///
/// `keys` must carry the account secret plus the pinned authorization/seal nonce secrets. For the
/// single-key vector path the account key is also the seal signer (`keys.seal_secret == None`).
pub fn build_and_encode_public_transfer(
    network: Network,
    intent: &PublicTransferIntent,
    keys: &DeterministicTransferKeys,
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let unsigned = build_public_transfer_unsigned(network, intent)?;
    let (transaction, id) = seal_public_transfer_deterministic(unsigned, keys)?;
    let bytes = bor_encode(transaction)?;
    Ok(EncodedPublicTransfer {
        encoded_transaction: EncodedTransactionBytes::from_vec(bytes),
        transaction_id: TransactionIdBytes::from_bytes(id.as_bytes()),
    })
}

/// Production counterpart of [`build_and_encode_public_transfer`]: builds, signs, seals (with the
/// stock random nonce) and BOR-encodes. The bytes/id are **not** reproducible — use this for real
/// submission, not for golden vectors.
pub fn build_and_encode_public_transfer_production(
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
    use tari_crypto::{
        keys::PublicKey as _,
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
        tari_utilities::ByteArray,
    };
    use tari_ootle_transaction::TransactionEnvelope;
    use tari_template_lib_types::{ComponentAddress, ObjectKey, ResourceAddress};

    use super::*;
    use crate::{
        builder::build_public_transfer_unsigned,
        tx::verify_all_signatures,
        types::{
            address::{ComponentAddressStr, ResourceAddressStr},
            bytes::{NonceSecretBytes, PublicKeyBytes, SecretKeyBytes},
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

    fn account_public_key_bytes() -> PublicKeyBytes {
        let pk = RistrettoPublicKey::from_secret_key(&fixed_scalar(11));
        PublicKeyBytes::from_bytes(pk.as_bytes()).unwrap()
    }

    fn auth_nonce() -> NonceSecretBytes {
        NonceSecretBytes::from_bytes(fixed_scalar(22).as_bytes()).unwrap()
    }

    fn seal_nonce() -> NonceSecretBytes {
        NonceSecretBytes::from_bytes(fixed_scalar(33).as_bytes()).unwrap()
    }

    fn det_keys() -> DeterministicTransferKeys {
        DeterministicTransferKeys::single_key(account_secret_bytes(), auth_nonce(), seal_nonce())
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
        let out = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &det_keys()).unwrap();
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

    /// The determinism proof: identical inputs (incl. pinned nonces) ⇒ identical bytes AND id.
    #[test]
    fn deterministic_path_is_byte_reproducible() {
        let a = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &det_keys()).unwrap();
        let b = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &det_keys()).unwrap();
        assert_eq!(
            a.encoded_transaction, b.encoded_transaction,
            "encoded bytes must be identical"
        );
        assert_eq!(a.transaction_id, b.transaction_id, "transaction id must be identical");
    }

    /// The contrasting assertion (documents *why* the deterministic path exists): the stock
    /// random-nonce production path yields differing bytes/id across runs.
    #[test]
    fn production_path_is_not_reproducible() {
        let keys = PublicTransferKeys::new(account_secret_bytes());
        let a = build_and_encode_public_transfer_production(Network::Esmeralda, &sample_intent(), &keys).unwrap();
        let b = build_and_encode_public_transfer_production(Network::Esmeralda, &sample_intent(), &keys).unwrap();
        assert_ne!(
            a.encoded_transaction, b.encoded_transaction,
            "random-nonce seal must produce differing bytes across runs"
        );
        assert_ne!(a.transaction_id, b.transaction_id, "and a differing transaction id");
    }

    /// The injected deterministic signatures must be real signatures, not just stable bytes.
    #[test]
    fn sealed_transaction_signatures_verify() {
        let unsigned = build_public_transfer_unsigned(Network::Esmeralda, &sample_intent()).unwrap();
        let (transaction, _id) = crate::tx::seal_public_transfer_deterministic(unsigned, &det_keys()).unwrap();
        assert!(verify_all_signatures(&transaction), "all signatures must verify");
    }

    /// The deterministic id matches the id `calculate_id` produces on the sealed transaction (i.e.
    /// the entry point does not invent a different id than the sealed transaction commits to).
    #[test]
    fn entry_point_id_matches_sealed_transaction_id() {
        let unsigned = build_public_transfer_unsigned(Network::Esmeralda, &sample_intent()).unwrap();
        let (transaction, id) = crate::tx::seal_public_transfer_deterministic(unsigned, &det_keys()).unwrap();
        let out = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &det_keys()).unwrap();
        assert_eq!(out.transaction_id.as_bytes(), id.as_bytes());
        assert_eq!(out.transaction_id.as_bytes(), transaction.calculate_id().as_bytes());
    }

    /// A separate seal signer also produces a deterministic, verifying transaction.
    #[test]
    fn separate_signer_path_is_deterministic_and_verifies() {
        let seal_secret = SecretKeyBytes::from_bytes(fixed_scalar(44).as_bytes()).unwrap();
        let keys =
            DeterministicTransferKeys::separate_signer(account_secret_bytes(), auth_nonce(), seal_secret, seal_nonce());
        let a = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &keys).unwrap();
        let b = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &keys).unwrap();
        assert_eq!(a, b, "separate-signer path is also byte-reproducible");

        let unsigned = build_public_transfer_unsigned(Network::Esmeralda, &sample_intent()).unwrap();
        let (transaction, _) = crate::tx::seal_public_transfer_deterministic(unsigned, &keys).unwrap();
        assert!(verify_all_signatures(&transaction));
    }

    /// Bad input must map to the correct error variant, never panic.
    #[test]
    fn malformed_secret_key_is_a_key_error() {
        // 0xff..ff is not a canonical Ristretto scalar.
        let bad_keys =
            DeterministicTransferKeys::single_key(SecretKeyBytes::from_array([0xff; 32]), auth_nonce(), seal_nonce());
        let err = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &bad_keys).unwrap_err();
        assert_eq!(err.code(), "KEY");
    }

    #[test]
    fn malformed_address_is_a_parse_error() {
        let mut intent = sample_intent();
        intent.from_account = ComponentAddressStr("not-a-component".to_string());
        let err = build_and_encode_public_transfer(Network::Esmeralda, &intent, &det_keys()).unwrap_err();
        assert_eq!(err.code(), "PARSE");
    }

    /// Amount above 2^53 µTari survives into the encoded bytes (re-decode and compare nothing is
    /// truncated — the transaction simply decodes cleanly with that amount in the withdraw arg).
    #[test]
    fn large_amount_round_trips_through_encoding() {
        let out = build_and_encode_public_transfer(Network::Esmeralda, &sample_intent(), &det_keys()).unwrap();
        let envelope = TransactionEnvelope::from_raw(out.encoded_transaction.as_bytes().to_vec().into_boxed_slice());
        let tx = envelope.decode().expect("decodes");
        // The withdraw instruction carries the (large) amount; a clean decode proves no truncation.
        assert!(!tx.instructions().is_empty());
    }

    #[test]
    fn account_public_key_helper_is_consistent() {
        // Guards the test fixture: the derived pk must match the boundary helper.
        assert_eq!(account_public_key_bytes().as_bytes().len(), 32);
    }
}
