//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Seal/encode a **resolved** [`PartialTransaction`].
//!
//! Input resolution produces a [`PartialTransaction`]
//! ([`build_public_transfer_unsigned_with_wants`] → [`apply_fetched_substates`]). This module takes a
//! fully-resolved partial all the way to submit-ready BOR bytes + transaction id by reusing the
//! seal/encode leaves ([`seal_public_transfer_deterministic`] / [`seal_public_transfer`] +
//! [`bor_encode`]), so the resolved path produces the same bytes as the explicit path.
//!
//! ## Canonical input sort
//!
//! The resolver accumulates inputs into a stable `Vec` in walk order, but the seal path owns the
//! canonical sort: before the bytes are signed/hashed, this module sorts the resolved inputs into a
//! deterministic order (by their canonical [`SubstateRequirement`] string form). Without this, two
//! resolutions that fold inputs in a different order would seal byte-divergent transactions. The sort
//! is applied only on the seal/encode path so the resolver's `into_unsigned` accumulation order stays
//! untouched.
//!
//! ## Three entry points
//!
//! - [`seal_and_encode_public_transfer`] — deterministic (vector) path: a resolved partial + pinned nonces ⇒
//!   byte-for-byte reproducible bytes + id.
//! - [`seal_and_encode_public_transfer_production`] — production (random-nonce) path.
//! - [`resolve_and_encode_public_transfer`] — convenience for the single-round case: build-with-wants → apply one
//!   fetched batch → seal/encode, returning a clear [`OotleSdkError::Resolution`] if the fetch left a required want
//!   unsatisfied.

use tari_ootle_transaction::UnsignedTransaction;

use crate::{
    FetchedSubstate,
    PartialTransaction,
    inputs::{Resolution, build_public_transfer_unsigned_with_wants},
    keys::{DeterministicTransferKeys, PublicTransferKeys},
    public_transfer::EncodedPublicTransfer,
    tx::{bor_encode, seal_public_transfer, seal_public_transfer_deterministic},
    types::{
        bytes::{EncodedTransactionBytes, TransactionIdBytes},
        error::OotleSdkError,
        intent::PublicTransferIntent,
        network::Network,
    },
};

/// Seals (deterministically, with pinned nonce secrets) and BOR-encodes a **resolved**
/// [`PartialTransaction`]. Byte-for-byte reproducible across calls with identical inputs — the
/// resolved-path counterpart of [`crate::build_and_encode_public_transfer`].
///
/// The partial must be fully resolved (no outstanding **required** wants). A partial still carrying
/// wants ⇒ [`OotleSdkError::Resolution`] (rather than silently sealing an incomplete input set).
pub fn seal_and_encode_public_transfer(
    partial: PartialTransaction,
    keys: &DeterministicTransferKeys,
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let unsigned = resolved_unsigned(partial)?;
    let (transaction, id) = seal_public_transfer_deterministic(unsigned, keys)?;
    encode(transaction, id)
}

/// Production counterpart of [`seal_and_encode_public_transfer`]: seals a resolved partial with the
/// stock random nonce. The bytes/id are **not** reproducible — use this for real submission.
pub fn seal_and_encode_public_transfer_production(
    partial: PartialTransaction,
    keys: &PublicTransferKeys,
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let unsigned = resolved_unsigned(partial)?;
    let (transaction, id) = seal_public_transfer(unsigned, keys)?;
    encode(transaction, id)
}

/// One-call convenience for the **single-round** resolution case: build the public-transfer unsigned
/// tx + want list, apply exactly one fetched batch, then seal/encode deterministically.
///
/// Intended for callers that have already fetched every wanted substate. If applying `fetched` does
/// not fully resolve the partial (a required want is still outstanding, e.g. the fetch was
/// insufficient) this returns
/// [`OotleSdkError::Resolution`] — it does **not** loop or panic. Multi-round resolution stays the
/// host's job via [`apply_fetched_substates`](crate::apply_fetched_substates).
pub fn resolve_and_encode_public_transfer(
    network: Network,
    intent: &PublicTransferIntent,
    fetched: &[FetchedSubstate],
    keys: &DeterministicTransferKeys,
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let (partial, _wants) = build_public_transfer_unsigned_with_wants(network, intent)?;
    match crate::apply_fetched_substates(partial, fetched)? {
        Resolution::Resolved(resolved) => seal_and_encode_public_transfer(resolved, keys),
        Resolution::NeedMore { want_list, .. } => Err(OotleSdkError::Resolution(format!(
            "single-round resolution incomplete: {} want(s) still outstanding after the fetched batch — fetch the \
             remaining substates and drive the multi-round loop via apply_fetched_substates",
            want_list.0.len(),
        ))),
    }
}

/// Folds a resolved partial into its [`UnsignedTransaction`] and applies the canonical input sort.
/// Errors if the partial is **not** fully resolved.
///
/// `pub(crate)` so the co-sign path derives the identical canonical unsigned transaction the seal
/// path seals.
pub(crate) fn resolved_unsigned(partial: PartialTransaction) -> Result<UnsignedTransaction, OotleSdkError> {
    if !partial.outstanding_wants().is_empty() {
        return Err(OotleSdkError::Resolution(format!(
            "cannot seal an unresolved transaction: {} want(s) still outstanding — call apply_fetched_substates until \
             it returns Resolved",
            partial.outstanding_wants().len(),
        )));
    }
    let mut unsigned = partial.into_unsigned();
    canonicalize_inputs(&mut unsigned);
    Ok(unsigned)
}

/// Canonically sorts the unsigned tx's inputs into a deterministic order, by the canonical
/// [`tari_ootle_common_types::SubstateRequirement`] string form (`<substate_id>:<version|?>`), which
/// is stable, total, and language-neutral.
///
/// This is the **only** place the resolved-path input order is fixed; the seal that follows hashes the
/// sorted set, so two resolutions of the same logical input set produce byte-identical bytes + id.
pub(crate) fn canonicalize_inputs(unsigned: &mut UnsignedTransaction) {
    let UnsignedTransaction::V1(v1) = unsigned;
    let mut inputs: Vec<_> = std::mem::take(&mut v1.inputs).into_iter().collect();
    inputs.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
    // The set we took out already had unique substate ids: `IndexSet` de-dups on `SubstateRequirement`'s
    // `Hash`/`Eq`, which ignore the version field, and `into_unsigned` only folds an input in when no entry with
    // the same substate id is already present. So re-collecting the sorted `Vec` into the `IndexSet` below cannot
    // silently drop a same-id/different-version entry. Assert that invariant so a future caller that breaks it
    // (e.g. mixing a versioned explicit input with an unversioned resolved input for the same id) fails loudly in
    // debug rather than silently keeping whichever version sorts first.
    debug_assert!(
        inputs.windows(2).all(|w| w[0].substate_id() != w[1].substate_id()),
        "canonicalize_inputs: duplicate substate_id after sort — version-silent IndexSet de-dup would occur"
    );
    v1.inputs = inputs.into_iter().collect();
}

/// BOR-encodes the sealed transaction into the [`EncodedPublicTransfer`] output.
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
        component::{Component, ComponentBody, ComponentHeader, derive_component_address_from_public_key},
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
        crypto::RistrettoPublicKeyBytes,
    };

    use super::*;
    use crate::{
        apply_fetched_substates,
        build_and_encode_public_transfer,
        tx::verify_all_signatures,
        types::{
            address::{ComponentAddressStr, ResourceAddressStr},
            bytes::{NonceSecretBytes, PublicKeyBytes, SecretKeyBytes},
            intent::{InputRef, TransferRecipient},
            numeric::BoundaryAmount,
        },
    };

    // --- Fixed, canonical test material ----------------------------------------------------------

    fn fixed_scalar(seed: u8) -> RistrettoSecretKey {
        let mut b = [0u8; 32];
        b[0] = seed;
        RistrettoSecretKey::from_canonical_bytes(&b).expect("low scalar is canonical")
    }

    fn account_secret_bytes() -> SecretKeyBytes {
        SecretKeyBytes::from_bytes(fixed_scalar(11).as_bytes()).unwrap()
    }

    fn det_keys() -> DeterministicTransferKeys {
        DeterministicTransferKeys::single_key(
            account_secret_bytes(),
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

    fn recipient_pk() -> RistrettoPublicKeyBytes {
        let pk = RistrettoPublicKey::from_secret_key(&fixed_scalar(7));
        RistrettoPublicKeyBytes::from_bytes(pk.as_bytes()).unwrap()
    }

    fn recipient_pk_bytes() -> PublicKeyBytes {
        PublicKeyBytes::from_bytes(recipient_pk().as_bytes()).unwrap()
    }

    fn recipient_component() -> ComponentAddress {
        derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &recipient_pk())
    }

    /// A resolved-path intent (no explicit inputs ⇒ the want set is derived). Amount above 2^53 µTari
    /// so the u64-safe path is exercised through resolution + seal too.
    fn resolved_intent() -> PublicTransferIntent {
        PublicTransferIntent {
            from_account: ComponentAddressStr::from_internal(&from_component()),
            recipient: TransferRecipient::PublicKey(recipient_pk_bytes()),
            resource_address: ResourceAddressStr::from_internal(&resource()),
            amount: BoundaryAmount::new((1u64 << 53) + 12_345),
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

    /// The full single-round fetched batch that resolves the 3-item want set: the from-component, the
    /// recipient component (absent — omitted), the from-vault. (Recipient component/vault are optional
    /// and created by `create_account_with_bucket` if absent.) This is two rounds in the generic loop,
    /// but the from-component reveals the from-vault, so one batch carrying both component + vault
    /// resolves it in a single `apply`.
    fn full_fetched_batch() -> Vec<FetchedSubstate> {
        vec![
            fetched(
                SubstateId::Component(from_component()),
                component_substate_json(&[from_vault_id()]),
            ),
            fetched(SubstateId::Vault(from_vault_id()), vault_substate_json()),
        ]
    }

    /// Drives the two-round resolution to a resolved partial for our fixture.
    fn resolved_partial() -> PartialTransaction {
        let (partial, _wants) =
            build_public_transfer_unsigned_with_wants(Network::Esmeralda, &resolved_intent()).unwrap();
        match apply_fetched_substates(partial, &full_fetched_batch()).unwrap() {
            Resolution::Resolved(p) => p,
            Resolution::NeedMore { want_list, .. } => panic!("expected Resolved, got NeedMore({want_list:?})"),
        }
    }

    // --- Tests -----------------------------------------------------------------------------------

    /// Two runs of the one-call convenience with the same fetched substates + pinned nonces ⇒
    /// identical bytes **and** identical id on the resolved path.
    #[test]
    fn deterministic_resolved_path_is_byte_reproducible() {
        let a = resolve_and_encode_public_transfer(
            Network::Esmeralda,
            &resolved_intent(),
            &full_fetched_batch(),
            &det_keys(),
        )
        .unwrap();
        let b = resolve_and_encode_public_transfer(
            Network::Esmeralda,
            &resolved_intent(),
            &full_fetched_batch(),
            &det_keys(),
        )
        .unwrap();
        assert_eq!(
            a.encoded_transaction, b.encoded_transaction,
            "encoded bytes must be identical"
        );
        assert_eq!(a.transaction_id, b.transaction_id, "transaction id must be identical");
    }

    /// The sealed resolved transaction's signatures must really verify (not just be stable bytes).
    #[test]
    fn resolved_sealed_signatures_verify() {
        let unsigned = resolved_unsigned(resolved_partial()).unwrap();
        let (transaction, _id) = seal_public_transfer_deterministic(unsigned, &det_keys()).unwrap();
        assert!(verify_all_signatures(&transaction), "all signatures must verify");
    }

    /// The id the entry point returns equals `calculate_id()` on the sealed resolved tx.
    #[test]
    fn resolved_id_matches_sealed_transaction_id() {
        let unsigned = resolved_unsigned(resolved_partial()).unwrap();
        let (transaction, id) = seal_public_transfer_deterministic(unsigned, &det_keys()).unwrap();
        let out = seal_and_encode_public_transfer(resolved_partial(), &det_keys()).unwrap();
        assert_eq!(out.transaction_id.as_bytes(), id.as_bytes());
        assert_eq!(out.transaction_id.as_bytes(), transaction.calculate_id().as_bytes());
    }

    /// Orchestration parity: the resolved path's sealed bytes equal the explicit path's bytes for the
    /// equivalent input set (after the canonical sort). This extends the unsigned-parity test to the
    /// fully sealed + encoded bytes.
    #[test]
    fn resolved_equals_explicit_for_equivalent_input_set() {
        // Resolved path → the from-component + the from-vault + the resource as inputs.
        let resolved = seal_and_encode_public_transfer(resolved_partial(), &det_keys()).unwrap();

        // Explicit path with the SAME inputs, in the canonical (sorted) order the seal path imposes.
        let mut explicit_intent = resolved_intent();
        let mut inputs = vec![
            InputRef::unversioned(SubstateId::Component(from_component()).to_string()),
            InputRef::unversioned(SubstateId::Vault(from_vault_id()).to_string()),
            InputRef::unversioned(SubstateId::Resource(resource()).to_string()),
        ];
        // build_and_encode_public_transfer does NOT sort, so supply them already canonically sorted
        // by the same canonical SubstateRequirement string form the seal path uses.
        let canonical = |s: &str| {
            use std::str::FromStr;
            tari_ootle_common_types::SubstateRequirement::unversioned(SubstateId::from_str(s).unwrap()).to_string()
        };
        inputs.sort_by(|a, b| canonical(&a.substate_id).cmp(&canonical(&b.substate_id)));
        explicit_intent.inputs = inputs;

        let explicit = build_and_encode_public_transfer(Network::Esmeralda, &explicit_intent, &det_keys()).unwrap();
        assert_eq!(
            resolved.encoded_transaction, explicit.encoded_transaction,
            "resolved and explicit sealed bytes must match for the equivalent input set"
        );
        assert_eq!(
            resolved.transaction_id, explicit.transaction_id,
            "and the transaction id"
        );
    }

    /// An insufficient fetch (the from-component never arrives, so the required from-vault want cannot
    /// resolve) maps to [`OotleSdkError::Resolution`] from the one-call convenience — never a panic.
    #[test]
    fn insufficient_fetch_is_a_resolution_error() {
        // Feed only the recipient component (irrelevant to the required from-vault want).
        let batch = vec![fetched(
            SubstateId::Component(recipient_component()),
            component_substate_json(&[]),
        )];
        let err = resolve_and_encode_public_transfer(Network::Esmeralda, &resolved_intent(), &batch, &det_keys())
            .unwrap_err();
        assert_eq!(err.code(), "RESOLUTION");
    }

    /// Sealing a still-unresolved partial is a `Resolution` error (guards the seal entry point itself,
    /// independent of the one-call convenience).
    #[test]
    fn sealing_unresolved_partial_is_a_resolution_error() {
        let (partial, _wants) =
            build_public_transfer_unsigned_with_wants(Network::Esmeralda, &resolved_intent()).unwrap();
        // Not resolved yet (no fetch applied).
        let err = seal_and_encode_public_transfer(partial, &det_keys()).unwrap_err();
        assert_eq!(err.code(), "RESOLUTION");
    }

    /// The production (random-nonce) resolved path produces a valid, decodable transaction (its bytes
    /// are intentionally non-reproducible).
    #[test]
    fn production_resolved_path_produces_valid_bytes() {
        let keys = PublicTransferKeys::new(account_secret_bytes());
        let out = seal_and_encode_public_transfer_production(resolved_partial(), &keys).unwrap();
        assert!(!out.encoded_transaction.as_bytes().is_empty());
        assert_eq!(out.transaction_id.as_bytes().len(), 32);
    }

    /// The canonical sort is order-independent: resolving the same logical input set must seal
    /// identically regardless of the order the resolver folds inputs.
    #[test]
    fn canonical_sort_makes_input_order_irrelevant() {
        // Round 1: fetch only the from-component (reveals the from-vault id).
        let (partial, _w) = build_public_transfer_unsigned_with_wants(Network::Esmeralda, &resolved_intent()).unwrap();
        let r1 = vec![fetched(
            SubstateId::Component(from_component()),
            component_substate_json(&[from_vault_id()]),
        )];
        let partial = match apply_fetched_substates(partial, &r1).unwrap() {
            Resolution::NeedMore { partial, .. } => partial,
            Resolution::Resolved(_) => panic!("expected NeedMore"),
        };
        // Round 2: fetch the from-vault.
        let r2 = vec![fetched(SubstateId::Vault(from_vault_id()), vault_substate_json())];
        let two_round = match apply_fetched_substates(partial, &r2).unwrap() {
            Resolution::Resolved(p) => seal_and_encode_public_transfer(p, &det_keys()).unwrap(),
            Resolution::NeedMore { .. } => panic!("expected Resolved"),
        };

        // Single-round path (component + vault in one batch).
        let single_round = seal_and_encode_public_transfer(resolved_partial(), &det_keys()).unwrap();

        assert_eq!(
            two_round, single_round,
            "the canonical sort must make multi-round vs single-round resolution seal identically"
        );
    }
}
