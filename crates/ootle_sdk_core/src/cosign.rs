//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Co-signing primitive: authorize → attach → seal across processes.
//!
//! Two-party (or N-party) flow, all crypto pure, only the transport is the host's job:
//!
//! 1. **Party A** builds and resolves a transaction to a [`PartialTransaction`], then derives the canonical
//!    [`UnsignedTransactionRecord`] and ships it to B. `PartialTransaction` is deliberately not serializable, so the
//!    [`UnsignedTransaction`] inside it is what crosses the wire.
//! 2. **Party B** re-derives the identical authorization signing message from that record plus A's seal public key,
//!    signs it with B's secret, and returns an [`Authorization`] (B's public key + the Schnorr signature). Signing uses
//!    either a pinned nonce (deterministic) or a fresh random nonce (production).
//! 3. **Party A** attaches the collected authorizations and seals via [`seal_and_encode_with_auth`] /
//!    [`seal_and_encode_with_auth_production`].
//!
//! ## Why A and B compute the same message
//!
//! The seal path applies a canonical input sort. If A shipped the unsorted unsigned tx, B would sign
//! a digest over a different input order than A seals over, and the engine would reject the seal (the
//! auth signature wouldn't verify against the sealed body). So [`unsigned_record_for_cosign`] returns
//! the already-canonicalized unsigned tx — the exact bytes A will seal — and B signs over that. The
//! seal entry points re-derive the same canonical unsigned from the partial, so A seals
//! byte-identically to what B signed.
//!
//! ## `is_seal_signer_authorized`
//!
//! With auth signatures present the seal keeps `is_seal_signer_authorized = false`; it is only `true`
//! when there are zero authorizations. An empty authorization slice therefore behaves like the plain
//! single-key seal.
//!
//! ## Secrets
//!
//! `signer_secret` is a [`SecretKeyBytes`] (`ZeroizeOnDrop`). The produced [`Authorization`] carries
//! only the signer's public key and the signature — no secret material.

use serde::{Deserialize, Serialize};
use tari_crypto::ristretto::{RistrettoSchnorr, RistrettoSecretKey};
use tari_ootle_transaction::{TransactionSignature, UnsignedTransaction};

use crate::{
    PartialTransaction,
    keys::{
        DeterministicTransferKeys,
        PublicTransferKeys,
        parse_nonce_secret,
        parse_secret_key,
        public_key_bytes_from_secret,
    },
    public_transfer::EncodedPublicTransfer,
    resolved_transfer::{canonicalize_inputs, resolved_unsigned},
    tx::{
        bor_encode,
        cosign_auth_message,
        seal_public_transfer_with_auth,
        seal_public_transfer_with_auth_deterministic,
    },
    types::{
        bytes::{EncodedTransactionBytes, PublicKeyBytes, SecretKeyBytes, SignatureBytes, TransactionIdBytes},
        error::OotleSdkError,
    },
};

/// The serializable unsigned-transaction record party A ships to party B.
///
/// This is a thin, typed wrapper over the canonical [`UnsignedTransaction`] (which already derives
/// `Serialize`/`Deserialize`). Wrapping it keeps the co-sign hand-off a named, versioned contract
/// rather than a bare engine type, with a stable JSON shape `{ "unsigned": <UnsignedTransaction JSON> }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsignedTransactionRecord {
    /// The canonical unsigned transaction A will seal — the exact value B's authorization signs over.
    pub unsigned: UnsignedTransaction,
}

impl UnsignedTransactionRecord {
    /// Wraps a canonical [`UnsignedTransaction`].
    pub fn new(unsigned: UnsignedTransaction) -> Self {
        Self { unsigned }
    }

    /// Borrows the inner unsigned transaction.
    pub fn unsigned(&self) -> &UnsignedTransaction {
        &self.unsigned
    }
}

/// One co-signer's authorization: the signer's public key and the Schnorr signature over the auth
/// message (which commits to A's seal public key + the unsigned tx).
///
/// Carries no secret material. Serializes as `{ "public_key": "<hex>", "signature": "<hex>" }`
/// (lowercase hex).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authorization {
    /// The co-signer's public key (the key that authorizes; not the seal signer).
    pub public_key: PublicKeyBytes,
    /// The Schnorr signature over the authorization message.
    pub signature: SignatureBytes,
}

impl Authorization {
    /// Reconstructs the internal [`TransactionSignature`] this authorization carries (for attaching to
    /// the unsealed transaction at seal time).
    fn to_internal(&self) -> Result<TransactionSignature, OotleSdkError> {
        use ootle_byte_type::ConvertFromByteType;
        use tari_template_lib_types::crypto::SchnorrSignatureBytes;

        let pk = self.public_key.to_internal();
        let sig_bytes = SchnorrSignatureBytes::from_bytes(self.signature.as_bytes())
            .map_err(|e| OotleSdkError::Key(format!("invalid authorization signature: {e}")))?;
        // Validate the signature bytes decode into a real Schnorr signature up front (a wrong width /
        // non-canonical scalar surfaces as KEY here rather than a verify failure at seal time).
        RistrettoSchnorr::convert_from_byte_type(&sig_bytes)
            .map_err(|e| OotleSdkError::Key(format!("invalid authorization signature: {e}")))?;
        Ok(TransactionSignature::new(pk, sig_bytes))
    }
}

/// Derives the canonical [`UnsignedTransactionRecord`] party A ships to party B.
///
/// **Borrows** the partial (it does not consume it) so A can ship the record *and* keep the partial to
/// seal — the seal path re-derives the identical canonical unsigned via [`resolved_unsigned`], so A and
/// B agree.
///
/// The partial must be fully resolved (no outstanding required wants — same rule as the seal path);
/// an unresolved partial ⇒ [`OotleSdkError::Resolution`]. The returned record carries the
/// already-canonicalized unsigned tx (the exact bytes the seal will sign over), so B's authorization
/// signs the identical message A seals.
///
/// A co-sign hand-off always attaches at least one authorization, so the record's
/// `is_seal_signer_authorized` flag is set to `false` here (the value the seal will commit to once
/// authorizations are present). B signs over this `false`, and the seal-with-auth path keeps it
/// `false` — so A and B commit to the identical flag value and the signatures cross-verify. Were the
/// flag left at the builder default `true`, B would sign over `true` while the seal flips it to
/// `false`, and B's authorization would no longer verify.
pub fn unsigned_record_for_cosign(partial: &PartialTransaction) -> Result<UnsignedTransactionRecord, OotleSdkError> {
    if !partial.outstanding_wants().is_empty() {
        return Err(OotleSdkError::Resolution(format!(
            "cannot derive a co-sign record from an unresolved transaction: {} want(s) still outstanding — call \
             apply_fetched_substates until it returns Resolved",
            partial.outstanding_wants().len(),
        )));
    }
    let mut unsigned = partial.cloned_unsigned();
    // Apply the same canonical input sort the seal path applies, so the shipped record is
    // byte-identical to what the seal will sign over.
    canonicalize_inputs(&mut unsigned);
    Ok(UnsignedTransactionRecord::new(
        unsigned.disabled_authorized_sealed_signer(),
    ))
}

/// **Party B — production path.** Authorizes A's unsigned transaction with a fresh random Schnorr
/// nonce, committing to A's `seal_public_key`. Returns B's [`Authorization`].
///
/// This is the only RNG site on the co-sign path. Bad key/nonce hex ⇒ [`OotleSdkError::Key`].
pub fn add_signature_production(
    unsigned: &UnsignedTransactionRecord,
    seal_public_key: &PublicKeyBytes,
    signer_secret: &SecretKeyBytes,
) -> Result<Authorization, OotleSdkError> {
    let secret = parse_secret_key(signer_secret)?;
    let message = cosign_auth_message(&seal_public_key.to_internal(), &unsigned.unsigned);
    let sig = RistrettoSchnorr::sign(&secret, message, &mut rand::rng())
        .map_err(|e| OotleSdkError::Key(format!("co-sign signing failed: {e}")))?;
    Ok(authorization_from_parts(&secret, sig))
}

/// **Party B — deterministic path.** Authorizes A's unsigned transaction with a **pinned** nonce
/// secret (no RNG), committing to A's `seal_public_key`.
///
/// With identical inputs this produces a byte-identical [`Authorization`]. Bad key/nonce hex ⇒
/// [`OotleSdkError::Key`].
pub fn add_signature(
    unsigned: &UnsignedTransactionRecord,
    seal_public_key: &PublicKeyBytes,
    signer_secret: &SecretKeyBytes,
    nonce: &crate::types::bytes::NonceSecretBytes,
) -> Result<Authorization, OotleSdkError> {
    let secret = parse_secret_key(signer_secret)?;
    let nonce = parse_nonce_secret(nonce)?;
    let message = cosign_auth_message(&seal_public_key.to_internal(), &unsigned.unsigned);
    let sig = RistrettoSchnorr::sign_with_nonce_and_message(&secret, nonce, &message)
        .map_err(|e| OotleSdkError::Key(format!("deterministic co-sign signing failed: {e}")))?;
    Ok(authorization_from_parts(&secret, sig))
}

/// Packs a signer secret + raw Schnorr signature into the boundary [`Authorization`].
fn authorization_from_parts(secret: &RistrettoSecretKey, sig: RistrettoSchnorr) -> Authorization {
    use ootle_byte_type::ToByteType;

    let public_key = public_key_bytes_from_secret(secret);
    let sig_bytes = sig.to_byte_type();
    Authorization {
        // RistrettoPublicKeyBytes is always 32 bytes; PublicKeyBytes::from_internal cannot mismatch.
        public_key: PublicKeyBytes::from_internal(&public_key),
        signature: SignatureBytes::from_bytes(&sig_bytes.to_bytes()),
    }
}

/// **Party A — deterministic path.** Attaches the supplied `authorizations` to a resolved partial and
/// seals it with the **pinned** seal nonce (byte-for-byte reproducible).
///
/// `is_seal_signer_authorized` stays `false` with authorizations present; an empty slice behaves like
/// the plain single-key seal. An unresolved partial ⇒ [`OotleSdkError::Resolution`].
pub fn seal_and_encode_with_auth(
    partial: PartialTransaction,
    keys: &DeterministicTransferKeys,
    authorizations: &[Authorization],
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let unsigned = resolved_unsigned(partial)?;
    let auth_signatures = internal_signatures(authorizations)?;
    let (transaction, id) = seal_public_transfer_with_auth_deterministic(unsigned, keys, auth_signatures)?;
    encode(transaction, id)
}

/// **Party A — production path.** Production (random-nonce) counterpart of
/// [`seal_and_encode_with_auth`]. The bytes/id are **not** reproducible — use for real submission.
pub fn seal_and_encode_with_auth_production(
    partial: PartialTransaction,
    keys: &PublicTransferKeys,
    authorizations: &[Authorization],
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let unsigned = resolved_unsigned(partial)?;
    let auth_signatures = internal_signatures(authorizations)?;
    let (transaction, id) = seal_public_transfer_with_auth(unsigned, keys, auth_signatures)?;
    encode(transaction, id)
}

/// Reconstructs the internal authorization signatures, preserving order.
fn internal_signatures(authorizations: &[Authorization]) -> Result<Vec<TransactionSignature>, OotleSdkError> {
    authorizations.iter().map(Authorization::to_internal).collect()
}

/// BOR-encodes the sealed transaction into an [`EncodedPublicTransfer`] (the same shape every seal
/// path returns).
fn encode(
    transaction: tari_ootle_transaction::Transaction,
    id: tari_ootle_transaction::TransactionId,
) -> Result<EncodedPublicTransfer, OotleSdkError> {
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
    use tari_engine_types::{
        component::{Component, ComponentBody, ComponentHeader},
        resource_container::ResourceContainer,
        substate::{SubstateId, SubstateValue},
        vault::Vault,
    };
    use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
    use tari_template_lib_types::{
        Amount,
        ComponentAddress,
        EntityId,
        ObjectKey,
        ResourceAddress,
        SubstateOwnerRule,
        VaultId,
        access_rules::ComponentAccessRules,
    };

    use super::*;
    use crate::{
        FetchedSubstate,
        Resolution,
        apply_fetched_substates,
        build_public_transfer_unsigned_with_wants,
        types::{
            address::{ComponentAddressStr, ResourceAddressStr},
            bytes::NonceSecretBytes,
            intent::{PublicTransferIntent, TransferRecipient},
            network::Network,
            numeric::BoundaryAmount,
        },
    };

    fn fixed_scalar(seed: u8) -> RistrettoSecretKey {
        let mut b = [0u8; 32];
        b[0] = seed;
        RistrettoSecretKey::from_canonical_bytes(&b).expect("low scalar is canonical")
    }

    // Party A's account key (the seal signer).
    fn a_secret_bytes() -> SecretKeyBytes {
        SecretKeyBytes::from_bytes(fixed_scalar(11).as_bytes()).unwrap()
    }

    fn a_seal_pk() -> PublicKeyBytes {
        let pk = RistrettoPublicKey::from_secret_key(&fixed_scalar(11));
        PublicKeyBytes::from_bytes(pk.as_bytes()).unwrap()
    }

    // Party B's co-signer key.
    fn b_secret_bytes() -> SecretKeyBytes {
        SecretKeyBytes::from_bytes(fixed_scalar(99).as_bytes()).unwrap()
    }

    fn b_nonce() -> NonceSecretBytes {
        NonceSecretBytes::from_bytes(fixed_scalar(77).as_bytes()).unwrap()
    }

    fn a_keys() -> DeterministicTransferKeys {
        DeterministicTransferKeys::single_key(
            a_secret_bytes(),
            NonceSecretBytes::from_bytes(fixed_scalar(22).as_bytes()).unwrap(),
            NonceSecretBytes::from_bytes(fixed_scalar(33).as_bytes()).unwrap(),
        )
    }

    fn from_component() -> ComponentAddress {
        ComponentAddress::new(ObjectKey::from_array([0xaa; ObjectKey::LENGTH]))
    }

    fn resource() -> ResourceAddress {
        ResourceAddress::new(ObjectKey::from_array([0xbb; ObjectKey::LENGTH]))
    }

    fn from_vault_id() -> VaultId {
        VaultId::new(ObjectKey::from_array([0xcc; ObjectKey::LENGTH]))
    }

    fn recipient_pk_bytes() -> PublicKeyBytes {
        let pk = RistrettoPublicKey::from_secret_key(&fixed_scalar(7));
        PublicKeyBytes::from_bytes(pk.as_bytes()).unwrap()
    }

    fn intent() -> PublicTransferIntent {
        PublicTransferIntent {
            from_account: ComponentAddressStr::from_internal(&from_component()),
            recipient: TransferRecipient::PublicKey(recipient_pk_bytes()),
            resource_address: ResourceAddressStr::from_internal(&resource()),
            amount: BoundaryAmount::new(1_000_000),
            fee: BoundaryAmount::new(2000),
            inputs: vec![],
            min_epoch: None,
            max_epoch: None,
            dry_run: false,
        }
    }

    fn component_substate_json(vault_ids: &[VaultId]) -> serde_json::Value {
        let state = tari_bor::to_value(&vault_ids.to_vec()).unwrap();
        let component = Component {
            header: ComponentHeader {
                template_address: ACCOUNT_TEMPLATE_ADDRESS,
                owner_rule: SubstateOwnerRule::None,
                access_rules: ComponentAccessRules::new(),
                entity_id: EntityId::from_array([0u8; EntityId::LENGTH]),
            },
            body: ComponentBody::from_cbor_value(state),
        };
        serde_json::to_value(SubstateValue::Component(component)).unwrap()
    }

    fn vault_substate_json() -> serde_json::Value {
        let vault = Vault::new(ResourceContainer::public_fungible(resource(), Amount::new(1_000_000)));
        serde_json::to_value(SubstateValue::Vault(vault)).unwrap()
    }

    fn fetched(id: impl ToString, value: serde_json::Value) -> FetchedSubstate {
        FetchedSubstate {
            substate_id: id.to_string(),
            version: 0,
            substate_value: value,
        }
    }

    fn resolved_partial() -> PartialTransaction {
        let (partial, _wants) = build_public_transfer_unsigned_with_wants(Network::Esmeralda, &intent()).unwrap();
        let batch = vec![
            fetched(
                SubstateId::Component(from_component()),
                component_substate_json(&[from_vault_id()]),
            ),
            fetched(SubstateId::Vault(from_vault_id()), vault_substate_json()),
        ];
        match apply_fetched_substates(partial, &batch).unwrap() {
            Resolution::Resolved(p) => p,
            Resolution::NeedMore { want_list, .. } => panic!("expected Resolved, got NeedMore({want_list:?})"),
        }
    }

    /// A and B derive the identical authorization signing message from the shipped unsigned record +
    /// A's seal public key.
    #[test]
    fn a_and_b_derive_the_same_signing_message() {
        let record = unsigned_record_for_cosign(&resolved_partial()).unwrap();
        // A computes it from the partial-derived unsigned; B reconstructs the record from JSON.
        let shipped = serde_json::to_string(&record).unwrap();
        let received: UnsignedTransactionRecord = serde_json::from_str(&shipped).unwrap();

        let a_msg = cosign_auth_message(&a_seal_pk().to_internal(), record.unsigned());
        let b_msg = cosign_auth_message(&a_seal_pk().to_internal(), received.unsigned());
        assert_eq!(a_msg, b_msg, "A and B must compute the identical signing message");
    }

    /// B's authorization verifies against B's public key over the shared message.
    #[test]
    fn authorization_verifies_against_signer_public_key() {
        use ootle_byte_type::{ConvertFromByteType, FromByteType};
        use tari_template_lib_types::crypto::SchnorrSignatureBytes;

        let record = unsigned_record_for_cosign(&resolved_partial()).unwrap();
        let auth = add_signature(&record, &a_seal_pk(), &b_secret_bytes(), &b_nonce()).unwrap();

        let message = cosign_auth_message(&a_seal_pk().to_internal(), record.unsigned());
        let pk = auth.public_key.to_internal().try_from_byte_type().unwrap();
        let sig = RistrettoSchnorr::convert_from_byte_type(
            &SchnorrSignatureBytes::from_bytes(auth.signature.as_bytes()).unwrap(),
        )
        .unwrap();
        assert!(sig.verify(&pk, message), "B's authorization must verify");
        // And it is B's key, not A's.
        let b_pk = RistrettoPublicKey::from_secret_key(&fixed_scalar(99));
        assert_eq!(auth.public_key.as_bytes(), b_pk.as_bytes());
    }

    /// Full authorize → attach → seal: the sealed cosigned transaction validates (all signatures
    /// verify), and `is_seal_signer_authorized` is `false` with an authorization attached.
    #[test]
    fn cosigned_seal_validates_and_flips_authorized_flag() {
        let record = unsigned_record_for_cosign(&resolved_partial()).unwrap();
        let auth = add_signature(&record, &a_seal_pk(), &b_secret_bytes(), &b_nonce()).unwrap();
        let out = seal_and_encode_with_auth(resolved_partial(), &a_keys(), std::slice::from_ref(&auth)).unwrap();

        // Decode + verify-all-signatures via the canonicalizer (errors if any sig fails).
        let value =
            crate::decode_and_canonicalize_sealed_transfer(Network::Esmeralda, &out.encoded_transaction.to_hex())
                .expect("cosigned sealed bytes must decode and verify");
        // With an authorization attached, is_seal_signer_authorized must be false.
        let flag = find_bool(&value, "is_seal_signer_authorized");
        assert_eq!(
            flag,
            Some(false),
            "is_seal_signer_authorized must be false with auth sigs present"
        );
    }

    /// An empty authorization slice behaves like the plain single-key seal — and there flips
    /// `is_seal_signer_authorized` to `true`.
    #[test]
    fn empty_authorizations_behave_like_plain_seal() {
        let out = seal_and_encode_with_auth(resolved_partial(), &a_keys(), &[]).unwrap();
        let plain = crate::seal_and_encode_public_transfer(resolved_partial(), &a_keys()).unwrap();
        // Plain seal also produces one auth signature (A's own); the no-auth cosign seal produces
        // zero. They are NOT byte-equal, but the no-auth cosign seal must still validate and set the
        // authorized flag true.
        let value =
            crate::decode_and_canonicalize_sealed_transfer(Network::Esmeralda, &out.encoded_transaction.to_hex())
                .expect("no-auth seal must validate");
        assert_eq!(find_bool(&value, "is_seal_signer_authorized"), Some(true));
        assert!(!plain.encoded_transaction.as_bytes().is_empty());
    }

    /// A tampered authorization (signature bytes mangled) ⇒ the sealed tx fails verification.
    #[test]
    fn tampered_authorization_is_rejected() {
        let record = unsigned_record_for_cosign(&resolved_partial()).unwrap();
        let mut auth = add_signature(&record, &a_seal_pk(), &b_secret_bytes(), &b_nonce()).unwrap();
        // Flip a byte in the signature scalar (still decodes as a Schnorr sig shape, but won't verify).
        let mut sig = auth.signature.as_bytes().to_vec();
        sig[10] ^= 0xff;
        auth.signature = SignatureBytes::from_bytes(&sig);

        match seal_and_encode_with_auth(resolved_partial(), &a_keys(), std::slice::from_ref(&auth)) {
            // Either attaching rejects the non-canonical signature (KEY), or the seal succeeds but the
            // verification step rejects it (VALIDATION). Both are correct "tampered ⇒ rejected".
            Err(e) => assert!(matches!(e.code(), "KEY" | "VALIDATION")),
            Ok(out) => {
                let err = crate::decode_and_canonicalize_sealed_transfer(
                    Network::Esmeralda,
                    &out.encoded_transaction.to_hex(),
                )
                .unwrap_err();
                assert_eq!(
                    err.code(),
                    "VALIDATION",
                    "tampered authorization must fail verification"
                );
            },
        }
    }

    /// The deterministic co-sign + seal path is byte-for-byte reproducible across two runs.
    #[test]
    fn deterministic_cosign_is_reproducible() {
        let record = unsigned_record_for_cosign(&resolved_partial()).unwrap();
        let auth_a = add_signature(&record, &a_seal_pk(), &b_secret_bytes(), &b_nonce()).unwrap();
        let auth_b = add_signature(&record, &a_seal_pk(), &b_secret_bytes(), &b_nonce()).unwrap();
        assert_eq!(
            auth_a, auth_b,
            "deterministic authorization must be identical across runs"
        );

        let seal_a = seal_and_encode_with_auth(resolved_partial(), &a_keys(), std::slice::from_ref(&auth_a)).unwrap();
        let seal_b = seal_and_encode_with_auth(resolved_partial(), &a_keys(), std::slice::from_ref(&auth_b)).unwrap();
        assert_eq!(
            seal_a, seal_b,
            "deterministic cosigned seal must be byte-identical across runs"
        );
    }

    /// Bad signer secret hex ⇒ `KEY`.
    #[test]
    fn bad_signer_secret_is_a_key_error() {
        let record = unsigned_record_for_cosign(&resolved_partial()).unwrap();
        let bad = SecretKeyBytes::from_array([0xff; 32]); // not a canonical Ristretto scalar
        let err = add_signature(&record, &a_seal_pk(), &bad, &b_nonce()).unwrap_err();
        assert_eq!(err.code(), "KEY");
    }

    /// Sealing an unresolved partial ⇒ `RESOLUTION` (guards the seal-with-auth entry point).
    #[test]
    fn unresolved_partial_is_a_resolution_error() {
        let (partial, _wants) = build_public_transfer_unsigned_with_wants(Network::Esmeralda, &intent()).unwrap();
        let err = seal_and_encode_with_auth(partial, &a_keys(), &[]).unwrap_err();
        assert_eq!(err.code(), "RESOLUTION");
    }

    /// Recursively finds the first boolean value for `key` in a JSON tree.
    fn find_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Bool(b)) = map.get(key) {
                    return Some(*b);
                }
                map.values().find_map(|v| find_bool(v, key))
            },
            serde_json::Value::Array(items) => items.iter().find_map(|v| find_bool(v, key)),
            _ => None,
        }
    }
}
