//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Stealth **input** resolution — fetches each input UTXO and decrypts it to recover its spend mask.
//! Pure and synchronous.
//!
//! Stealth inputs are on-chain **UTXO substates** (`UtxoAddress::new(resource, commitment)` →
//! `SubstateId`). The spend mask is **not** on the wire — the resolver must **fetch** the UTXO and
//! **decrypt** it with the owner account's view-only secret to recover the `(value, mask)` pair
//! (two-phase fetch + decrypt-in-core). This module extends the two-phase machinery in
//! [`crate::inputs`] with:
//!
//! - [`stealth_utxo_substate_id`] — derives the canonical `UtxoAddress` substate id string for a `(resource_address,
//!   commitment)` pair (the host fetches it like any other substate).
//! - [`resolve_one_stealth_utxo`] — the per-want resolve arm: validate the fetched UTXO (not frozen, not burnt),
//!   recover its `public_nonce`, classify the signer (seal vs required vs account-key), and decrypt to recover the
//!   mask.
//! - [`spend_secrets_map`] — zips the intent's stealth inputs with the caller-supplied view-only secrets into the
//!   `account_pk_hex → secret` map the resolver consumes.
//! - [`WantList::from_stealth_inputs`] — emits one [`WantItem::StealthUtxo`] per stealth input.
//!
//! The resolve recipe is: fetch the UTXO → recover its sender nonce → classify the signer → decrypt
//! the mask. The only I/O (the fetch) is driven by the host; the decrypt is pure crypto
//! ([`StealthCryptoApi::decrypt_utxo_data`]) — no RNG.
//!
//! ## Secrecy
//!
//! The `spend_secrets` map is **borrowed** for the duration of resolution and never stored in the
//! [`PartialTransaction`]. The recovered aggregate mask *is* stored (the balance proof needs it) —
//! and it, like the per-input witness masks, is **zeroized on drop** because the underlying
//! `RistrettoSecretKey` derives `ZeroizeOnDrop`. The borrowed `SecretKeyBytes` secrets likewise wipe
//! themselves on drop (their newtype `ZeroizeOnDrop`s — see [`crate::types::bytes`]).

use std::collections::HashMap;

use tari_crypto::{
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_engine_types::substate::{SubstateId, SubstateValue};
use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_wallet_crypto::{MaskAndValue, StealthCryptoApi, StealthInputWitness};
use tari_template_lib_types::{ResourceAddress, UtxoAddress, crypto::PedersenCommitmentBytes};

use crate::{
    inputs::{StealthSignerEntry, WantItem, WantList},
    types::{bytes::SecretKeyBytes, error::OotleSdkError, stealth::StealthTransferIntent},
};

/// Derives the canonical `UtxoAddress` substate id string for a stealth input's
/// `(resource_address, commitment)` pair.
///
/// Builds `SubstateId::from(UtxoAddress::new(resource, commitment))`. The `resource_address` is a
/// canonical `resource_<hex>` string and `commitment` is 32-byte lowercase hex (64 chars).
pub fn stealth_utxo_substate_id(resource_address: &str, commitment: &str) -> Result<SubstateId, OotleSdkError> {
    use std::str::FromStr;

    let resource = ResourceAddress::from_str(resource_address)
        .map_err(|e| OotleSdkError::Parse(format!("invalid resource address '{resource_address}': {e}")))?;
    let commitment_bytes = PedersenCommitmentBytes::from_bytes(
        &hex::decode(commitment).map_err(|e| OotleSdkError::Parse(format!("invalid commitment hex: {e}")))?,
    )
    .map_err(|e| OotleSdkError::Parse(format!("invalid commitment bytes: {e}")))?;

    Ok(SubstateId::from(UtxoAddress::new(resource, commitment_bytes.into())))
}

impl WantList {
    /// Produces one [`WantItem::StealthUtxo`] per stealth input in `intent` (resource from the
    /// intent, commitment + owner account pk from each [`StealthInputSpec`](crate::types::stealth::StealthInputSpec)).
    /// Stealth inputs are always `required: true` — a missing UTXO is an error.
    pub fn from_stealth_inputs(intent: &StealthTransferIntent) -> Self {
        let resource_address = intent.resource_address.as_str().to_string();
        let items = intent
            .inputs
            .iter()
            .map(|input| WantItem::StealthUtxo {
                resource_address: resource_address.clone(),
                commitment: input.commitment.to_hex(),
                owner_account_pk: input.owner_account_pk.to_hex(),
                required: true,
            })
            .collect();
        WantList(items)
    }

    /// The stealth want set for the live two-phase driver: the from-account component and its vault
    /// for the transfer resource, prepended to the per-input stealth-UTXO wants. The account is
    /// always referenced (the fee is paid from it via `pay_fee_from_component`, and a revealed input
    /// withdraws from it), so both must be resolved and declared as inputs or the engine rejects the
    /// tx with the component/vault "not found". [`WantList::from_stealth_inputs`] alone omits them —
    /// used only by the one-shot path, whose caller supplies every substate up front.
    pub fn from_stealth_inputs_with_account(intent: &StealthTransferIntent) -> Self {
        let from_account = intent.from_account.as_str().to_string();
        let mut items = vec![
            WantItem::SpecificSubstate {
                substate_id: from_account.clone(),
                required: true,
            },
            WantItem::VaultForResource {
                component_address: from_account,
                resource_address: intent.resource_address.as_str().to_string(),
                required: true,
            },
        ];
        items.extend(Self::from_stealth_inputs(intent).0);
        WantList(items)
    }
}

/// Builds the `account_pk_hex → view-only secret` map the stealth resolver consumes.
///
/// Zips `intent.inputs[i].owner_account_pk` with `secrets[i]` positionally; a length mismatch is an
/// [`OotleSdkError::Validation`]. The map is keyed by the owner account public key's **lowercase
/// hex** (matching [`WantItem::StealthUtxo::owner_account_pk`]), so the resolver can look up the
/// right secret regardless of input ordering.
///
/// The "spend secret" is the account's **view-only secret key** — both the internal
/// `decrypt_input_data` and [`StealthCryptoApi::decrypt_utxo_data`] derive the AEAD decryption key
/// from `(view_secret, output.public_nonce)`.
pub fn spend_secrets_map(
    intent: &StealthTransferIntent,
    secrets: &[SecretKeyBytes],
) -> Result<HashMap<String, SecretKeyBytes>, OotleSdkError> {
    if intent.inputs.len() != secrets.len() {
        return Err(OotleSdkError::Validation(format!(
            "spend secrets count ({}) must equal stealth input count ({})",
            secrets.len(),
            intent.inputs.len()
        )));
    }
    let mut map = HashMap::with_capacity(intent.inputs.len());
    for (input, secret) in intent.inputs.iter().zip(secrets.iter()) {
        map.insert(input.owner_account_pk.to_hex(), secret.clone());
    }
    // Two inputs sharing an owner pk but supplied *different* secrets would silently collapse to the
    // last one. Distinct owner pks are required so the per-input lookup is unambiguous.
    if map.len() != intent.inputs.len() {
        return Err(OotleSdkError::Validation(
            "duplicate owner_account_pk across stealth inputs — each input must carry a distinct owner account pk"
                .to_string(),
        ));
    }
    Ok(map)
}

/// Resolve arm for [`WantItem::StealthUtxo`].
///
/// Returns `Ok(true)` when the input is fully resolved (witness + mask accumulated, signer
/// classified, UTXO added as an input), `Ok(false)` when the UTXO still needs to be fetched. Any
/// validation/decryption failure is an error (stealth inputs are always required).
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_one_stealth_utxo(
    resource_address: &str,
    commitment: &str,
    owner_account_pk: &str,
    cache: &HashMap<SubstateId, Option<SubstateValue>>,
    spend_secrets: &HashMap<String, SecretKeyBytes>,
    witnesses: &mut Vec<StealthInputWitness>,
    agg_input_mask: &mut RistrettoSecretKey,
    must_sign_with_account_key: bool,
    seal_signer: &mut Option<StealthSignerEntry>,
    required_signers: &mut Vec<StealthSignerEntry>,
    resolved: &mut Vec<SubstateRequirement>,
    to_fetch: &mut Vec<SubstateId>,
) -> Result<bool, OotleSdkError> {
    // Derive the UTXO substate id.
    let utxo_substate_id = stealth_utxo_substate_id(resource_address, commitment)?;

    // Resolve it against the cache.
    let value = match cache.get(&utxo_substate_id) {
        // Never fetched → ask the host.
        None => {
            to_fetch.push(utxo_substate_id);
            return Ok(false);
        },
        // Definitively absent → a required stealth UTXO that does not exist is an error.
        Some(None) => {
            return Err(OotleSdkError::Invalid(format!(
                "stealth UTXO '{utxo_substate_id}' not found"
            )));
        },
        Some(Some(value)) => value,
    };

    // Validate the substate (UTXO, not frozen, not burnt) and recover the sender public nonce via the
    // shared decode ([`extract_utxo_output`]) — the same field extraction the receive path's
    // `decode_stealth_utxo` uses, so the two can never drift.
    let (public_nonce, output) = crate::stealth::decode::extract_utxo_output(value, &utxo_substate_id.to_string())?;

    // Look up the caller-supplied view-only secret for this input's owner account.
    let spend_secret = spend_secrets.get(owner_account_pk).ok_or_else(|| {
        OotleSdkError::Invalid(format!("no spend secret supplied for account pk '{owner_account_pk}'"))
    })?;
    let claim_secret = RistrettoSecretKey::from_canonical_bytes(spend_secret.as_bytes())
        .map_err(|e| OotleSdkError::Key(format!("invalid spend secret for account pk '{owner_account_pk}': {e}")))?;

    // Decrypt to recover the mask (skip_memo = true). Pure crypto.
    let commitment_bytes = derive_commitment_bytes(&utxo_substate_id)?;
    let decrypted = StealthCryptoApi::new()
        .decrypt_utxo_data(
            &output.output.encrypted_data,
            &commitment_bytes,
            &claim_secret,
            &public_nonce,
            true,
        )
        .map_err(|e| OotleSdkError::Key(format!("stealth UTXO decryption failed for '{utxo_substate_id}': {e}")))?;

    // Accumulate the per-input mask into the aggregate input mask.
    *agg_input_mask = &*agg_input_mask + decrypted.mask();

    // Classify the signer: the first non-account-key-signed input becomes the seal signer; all
    // subsequent ones (and all of them when account-key signing) are required signers.
    let entry = StealthSignerEntry {
        account_pk_hex: owner_account_pk.to_string(),
        public_nonce_bytes: public_nonce_to_bytes(&public_nonce)?,
    };
    if !must_sign_with_account_key && seal_signer.is_none() {
        *seal_signer = Some(entry);
    } else if !required_signers.contains(&entry) {
        required_signers.push(entry);
    } else {
        // Already a required signer — nothing to record.
    }

    // Record the recovered witness.
    witnesses.push(StealthInputWitness {
        mask_and_value: MaskAndValue {
            value: decrypted.value(),
            mask: decrypted.mask().clone(),
        },
        spend_condition: None,
    });

    // Add the UTXO substate as a transaction input.
    let req = SubstateRequirement::unversioned(utxo_substate_id);
    if !resolved.contains(&req) {
        resolved.push(req);
    }

    Ok(true)
}

/// Extracts the 32-byte commitment from a UTXO substate id (`UtxoAddress` → `UtxoId` →
/// `PedersenCommitmentBytes`).
fn derive_commitment_bytes(utxo_substate_id: &SubstateId) -> Result<PedersenCommitmentBytes, OotleSdkError> {
    let address = utxo_substate_id
        .as_utxo_address()
        .ok_or_else(|| OotleSdkError::Invalid(format!("'{utxo_substate_id}' is not a UTXO address")))?;
    Ok(address.into_contents().id.into_commitment_bytes())
}

/// Encodes a recovered public nonce as boundary public-key bytes for the signer entry.
fn public_nonce_to_bytes(
    public_nonce: &RistrettoPublicKey,
) -> Result<crate::types::bytes::PublicKeyBytes, OotleSdkError> {
    crate::types::bytes::PublicKeyBytes::from_bytes(public_nonce.as_bytes())
        .map_err(|e| OotleSdkError::Key(format!("public nonce is not 32 bytes: {e}")))
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey as _, SecretKey as _},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    };
    use tari_engine_types::{
        Utxo,
        UtxoOutput,
        crypto::{OutputBody, commit_u64_amount},
        substate::SubstateValue,
    };
    use tari_ootle_wallet_crypto::{encrypted_data::encrypt_data, kdfs};
    use tari_template_lib_types::{
        ObjectKey,
        ResourceAddress,
        access_rules::AccessRule,
        crypto::UtxoTag,
        stealth::SpendCondition,
    };

    use super::*;
    use crate::types::{
        address::ResourceAddressStr,
        bytes::PublicKeyBytes,
        numeric::BoundaryAmount,
        stealth::StealthInputSpec,
    };

    fn resource() -> ResourceAddress {
        ResourceAddress::new(ObjectKey::from_array([0xbb; ObjectKey::LENGTH]))
    }

    fn secret(seed: u8) -> RistrettoSecretKey {
        RistrettoSecretKey::from_uniform_bytes(&[seed; 64]).unwrap()
    }

    /// A fabricated stealth UTXO: a known `(value, mask)` encrypted to the recipient's view secret
    /// via a known sender nonce, wrapped in `SubstateValue::Utxo`. Returns the want, the substate
    /// value, the view secret (the "spend secret"), and the owner account pk hex used to key it.
    struct Fixture {
        want: WantItem,
        value_json: serde_json::Value,
        substate_id: SubstateId,
        view_secret: RistrettoSecretKey,
        owner_account_pk_hex: String,
        mask: RistrettoSecretKey,
        value: u64,
    }

    fn make_fixture(value: u64, mask_seed: u8, nonce_seed: u8, view_seed: u8, owner_seed: u8) -> Fixture {
        let mask = secret(mask_seed);
        let nonce_secret = secret(nonce_seed);
        let public_nonce = RistrettoPublicKey::from_secret_key(&nonce_secret);
        let view_secret = secret(view_seed);

        // Commitment of (mask, value); the UTXO substate id is derived from these bytes.
        let commitment = commit_u64_amount(&mask, value).to_byte_type();
        let commitment_hex = hex::encode(commitment.as_bytes());

        // Encrypt (value, mask) with the AEAD key derived from (view_secret, public_nonce).
        let encryption_key = kdfs::encrypted_data_dh_kdf_aead(&view_secret, &public_nonce);
        let encrypted_data = encrypt_data(value, &mask, &encryption_key, None).unwrap();

        let output_body = OutputBody {
            public_nonce: public_nonce.to_byte_type(),
            encrypted_data,
            minimum_value_promise: 0,
            viewable_balance: None,
        };
        let utxo = Utxo::new(UtxoOutput {
            output: output_body,
            spend_condition: SpendCondition::AccessRule(AccessRule::AllowAll),
            tag: UtxoTag::new(0),
        });

        let owner_account_pk = RistrettoPublicKey::from_secret_key(&secret(owner_seed));
        let owner_account_pk_hex = PublicKeyBytes::from_bytes(owner_account_pk.to_byte_type().as_bytes())
            .unwrap()
            .to_hex();

        let substate_id = stealth_utxo_substate_id(&resource().to_string(), &commitment_hex).unwrap();

        Fixture {
            want: WantItem::StealthUtxo {
                resource_address: resource().to_string(),
                commitment: commitment_hex,
                owner_account_pk: owner_account_pk_hex.clone(),
                required: true,
            },
            value_json: serde_json::to_value(SubstateValue::Utxo(utxo)).unwrap(),
            substate_id,
            view_secret,
            owner_account_pk_hex,
            mask,
            value,
        }
    }

    fn want_fields(want: &WantItem) -> (&str, &str, &str) {
        match want {
            WantItem::StealthUtxo {
                resource_address,
                commitment,
                owner_account_pk,
                ..
            } => (resource_address, commitment, owner_account_pk),
            _ => panic!("expected StealthUtxo"),
        }
    }

    /// Runs `resolve_one_stealth_utxo` for a single fixture with the cache pre-populated with the
    /// substate value (or `None` / a different value to exercise error paths).
    #[allow(clippy::type_complexity)]
    fn resolve(
        f: &Fixture,
        cache_value: Option<SubstateValue>,
        secrets: &HashMap<String, SecretKeyBytes>,
        must_sign_with_account_key: bool,
        seal_signer: &mut Option<StealthSignerEntry>,
        required_signers: &mut Vec<StealthSignerEntry>,
        witnesses: &mut Vec<StealthInputWitness>,
        agg_input_mask: &mut RistrettoSecretKey,
    ) -> Result<bool, OotleSdkError> {
        let mut cache: HashMap<SubstateId, Option<SubstateValue>> = HashMap::new();
        cache.insert(f.substate_id.clone(), cache_value);
        let (res, com, owner) = want_fields(&f.want);
        let mut resolved = Vec::new();
        let mut to_fetch = Vec::new();
        resolve_one_stealth_utxo(
            res,
            com,
            owner,
            &cache,
            secrets,
            witnesses,
            agg_input_mask,
            must_sign_with_account_key,
            seal_signer,
            required_signers,
            &mut resolved,
            &mut to_fetch,
        )
    }

    fn secrets_for(f: &Fixture) -> HashMap<String, SecretKeyBytes> {
        let mut m = HashMap::new();
        m.insert(
            f.owner_account_pk_hex.clone(),
            SecretKeyBytes::from_bytes(f.view_secret.as_bytes()).unwrap(),
        );
        m
    }

    // (a) WantItem::StealthUtxo derives the correct substate id.
    #[test]
    fn stealth_utxo_want_derives_utxo_substate_id() {
        let f = make_fixture(1000, 1, 2, 3, 4);
        let ids = f.want.seed_substate_ids();
        assert_eq!(ids.len(), 1);
        assert!(ids[0].starts_with("utxo_"), "expected utxo_ id, got {}", ids[0]);
        assert_eq!(ids[0], f.substate_id.to_string());
    }

    // (b) Serde round-trip of WantItem::StealthUtxo with the snake_case "stealth_utxo" discriminant.
    #[test]
    fn stealth_utxo_want_serde_round_trips() {
        let f = make_fixture(1000, 1, 2, 3, 4);
        let json = serde_json::to_string(&f.want).unwrap();
        assert!(
            json.contains("\"stealth_utxo\""),
            "discriminant must be stealth_utxo: {json}"
        );
        let back: WantItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f.want);
    }

    // (c) Happy decrypt → mask recovery.
    #[test]
    fn happy_decrypt_recovers_mask_and_value() {
        let f = make_fixture(12_345, 10, 11, 12, 13);
        let secrets = secrets_for(&f);
        let mut seal = None;
        let mut req = Vec::new();
        let mut witnesses = Vec::new();
        let mut mask = RistrettoSecretKey::default();
        let satisfied = resolve(
            &f,
            Some(serde_json::from_value(f.value_json.clone()).unwrap()),
            &secrets,
            false,
            &mut seal,
            &mut req,
            &mut witnesses,
            &mut mask,
        )
        .unwrap();
        assert!(satisfied);
        assert_eq!(witnesses.len(), 1);
        assert_eq!(witnesses[0].mask_and_value.value, f.value);
        assert_eq!(witnesses[0].mask_and_value.mask.as_bytes(), f.mask.as_bytes());
        // Aggregate mask == this single input's mask.
        assert_eq!(mask.as_bytes(), f.mask.as_bytes());
    }

    // (d) Frozen UTXO rejection.
    #[test]
    fn frozen_utxo_is_rejected() {
        let f = make_fixture(1000, 20, 21, 22, 23);
        let mut value: SubstateValue = serde_json::from_value(f.value_json.clone()).unwrap();
        if let SubstateValue::Utxo(u) = &mut value {
            u.freeze();
        }
        let secrets = secrets_for(&f);
        let err = resolve(
            &f,
            Some(value),
            &secrets,
            false,
            &mut None,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut RistrettoSecretKey::default(),
        )
        .unwrap_err();
        assert_eq!(err.code(), "INVALID");
        assert!(err.to_string().contains("frozen"));
    }

    // (e) Burnt UTXO rejection.
    #[test]
    fn burnt_utxo_is_rejected() {
        let f = make_fixture(1000, 30, 31, 32, 33);
        let mut value: SubstateValue = serde_json::from_value(f.value_json.clone()).unwrap();
        if let SubstateValue::Utxo(u) = &mut value {
            u.burn();
        }
        let secrets = secrets_for(&f);
        let err = resolve(
            &f,
            Some(value),
            &secrets,
            false,
            &mut None,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut RistrettoSecretKey::default(),
        )
        .unwrap_err();
        assert_eq!(err.code(), "INVALID");
        assert!(err.to_string().contains("burnt"));
    }

    // (f) Wrong spend secret (decryption fails) → KEY error.
    #[test]
    fn wrong_spend_secret_fails_decryption() {
        let f = make_fixture(1000, 40, 41, 42, 43);
        // Supply a different view secret for the same owner pk.
        let mut secrets = HashMap::new();
        secrets.insert(
            f.owner_account_pk_hex.clone(),
            SecretKeyBytes::from_bytes(secret(99).as_bytes()).unwrap(),
        );
        let err = resolve(
            &f,
            Some(serde_json::from_value(f.value_json.clone()).unwrap()),
            &secrets,
            false,
            &mut None,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut RistrettoSecretKey::default(),
        )
        .unwrap_err();
        assert_eq!(err.code(), "KEY");
    }

    // Missing spend secret for the owner pk → INVALID error.
    #[test]
    fn missing_spend_secret_is_invalid() {
        let f = make_fixture(1000, 44, 45, 46, 47);
        let err = resolve(
            &f,
            Some(serde_json::from_value(f.value_json.clone()).unwrap()),
            &HashMap::new(),
            false,
            &mut None,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut RistrettoSecretKey::default(),
        )
        .unwrap_err();
        assert_eq!(err.code(), "INVALID");
        assert!(err.to_string().contains("no spend secret"));
    }

    // Not-yet-fetched UTXO (cache None entry absent) → not satisfied, schedules a fetch.
    #[test]
    fn unfetched_utxo_schedules_fetch() {
        let f = make_fixture(1000, 48, 49, 50, 51);
        let secrets = secrets_for(&f);
        let cache: HashMap<SubstateId, Option<SubstateValue>> = HashMap::new(); // never fetched
        let (res, com, owner) = want_fields(&f.want);
        let mut to_fetch = Vec::new();
        let satisfied = resolve_one_stealth_utxo(
            res,
            com,
            owner,
            &cache,
            &secrets,
            &mut Vec::new(),
            &mut RistrettoSecretKey::default(),
            false,
            &mut None,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut to_fetch,
        )
        .unwrap();
        assert!(!satisfied);
        assert_eq!(to_fetch, vec![f.substate_id.clone()]);
    }

    // Definitively-absent required UTXO (cache Some(None)) → INVALID.
    #[test]
    fn absent_required_utxo_is_invalid() {
        let f = make_fixture(1000, 52, 53, 54, 55);
        let secrets = secrets_for(&f);
        let err = resolve(
            &f,
            None, // Some(None) in cache: fetched-but-absent
            &secrets,
            false,
            &mut None,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut RistrettoSecretKey::default(),
        )
        .unwrap_err();
        assert_eq!(err.code(), "INVALID");
        assert!(err.to_string().contains("not found"));
    }

    // (g) Signer classification — seal signer (first input, no revealed).
    #[test]
    fn first_input_no_revealed_is_seal_signer() {
        let f = make_fixture(1000, 60, 61, 62, 63);
        let secrets = secrets_for(&f);
        let mut seal = None;
        let mut req = Vec::new();
        resolve(
            &f,
            Some(serde_json::from_value(f.value_json.clone()).unwrap()),
            &secrets,
            false,
            &mut seal,
            &mut req,
            &mut Vec::new(),
            &mut RistrettoSecretKey::default(),
        )
        .unwrap();
        assert!(seal.is_some());
        assert!(req.is_empty());
        assert_eq!(seal.unwrap().account_pk_hex, f.owner_account_pk_hex);
    }

    // (h) Signer classification — second input becomes a required signer.
    #[test]
    fn second_input_is_required_signer() {
        let f1 = make_fixture(1000, 70, 71, 72, 73);
        let f2 = make_fixture(2000, 74, 75, 76, 77);
        let mut secrets = secrets_for(&f1);
        secrets.extend(secrets_for(&f2));
        let mut seal = None;
        let mut req = Vec::new();
        let mut witnesses = Vec::new();
        let mut mask = RistrettoSecretKey::default();

        resolve(
            &f1,
            Some(serde_json::from_value(f1.value_json.clone()).unwrap()),
            &secrets,
            false,
            &mut seal,
            &mut req,
            &mut witnesses,
            &mut mask,
        )
        .unwrap();
        resolve(
            &f2,
            Some(serde_json::from_value(f2.value_json.clone()).unwrap()),
            &secrets,
            false,
            &mut seal,
            &mut req,
            &mut witnesses,
            &mut mask,
        )
        .unwrap();

        assert_eq!(seal.as_ref().unwrap().account_pk_hex, f1.owner_account_pk_hex);
        assert_eq!(req.len(), 1);
        assert_eq!(req[0].account_pk_hex, f2.owner_account_pk_hex);
        // Aggregate mask is the sum of both input masks.
        let expected = &f1.mask + &f2.mask;
        assert_eq!(mask.as_bytes(), expected.as_bytes());
    }

    // (i) Signer classification — account-key path (revealed input present).
    #[test]
    fn revealed_input_forces_account_key_signer() {
        let f = make_fixture(1000, 80, 81, 82, 83);
        let secrets = secrets_for(&f);
        let mut seal = None;
        let mut req = Vec::new();
        resolve(
            &f,
            Some(serde_json::from_value(f.value_json.clone()).unwrap()),
            &secrets,
            true, // must_sign_with_account_key
            &mut seal,
            &mut req,
            &mut Vec::new(),
            &mut RistrettoSecretKey::default(),
        )
        .unwrap();
        assert!(seal.is_none());
        assert_eq!(req.len(), 1);
        assert_eq!(req[0].account_pk_hex, f.owner_account_pk_hex);
    }

    // (j) WantList::from_stealth_inputs derives one want per input.
    #[test]
    fn want_list_from_stealth_inputs() {
        let f1 = make_fixture(1000, 90, 91, 92, 93);
        let f2 = make_fixture(2000, 94, 95, 96, 97);
        let (_, com1, owner1) = want_fields(&f1.want);
        let (_, com2, owner2) = want_fields(&f2.want);

        let intent = StealthTransferIntent {
            from_account: crate::types::address::ComponentAddressStr::parse(
                tari_template_lib_types::ComponentAddress::new(ObjectKey::from_array([0xaa; ObjectKey::LENGTH]))
                    .to_string(),
            )
            .unwrap(),
            resource_address: ResourceAddressStr::parse(resource().to_string()).unwrap(),
            fee: BoundaryAmount::new(2000),
            inputs: vec![
                StealthInputSpec {
                    commitment: crate::types::stealth::CommitmentBytes::from_hex(com1).unwrap(),
                    owner_account_pk: PublicKeyBytes::from_hex(owner1).unwrap(),
                },
                StealthInputSpec {
                    commitment: crate::types::stealth::CommitmentBytes::from_hex(com2).unwrap(),
                    owner_account_pk: PublicKeyBytes::from_hex(owner2).unwrap(),
                },
            ],
            outputs: vec![],
            revealed_input_amount: 0,
            revealed_output_amount: 0,
            min_epoch: None,
            max_epoch: None,
            dry_run: false,
            pay_fee_from_revealed: false,
        };

        let wants = WantList::from_stealth_inputs(&intent);
        assert_eq!(wants.0.len(), 2);
        assert_eq!(wants.0[0], f1.want);
        assert_eq!(wants.0[1], f2.want);

        // spend_secrets_map zips inputs with secrets positionally.
        let secrets = vec![
            SecretKeyBytes::from_bytes(f1.view_secret.as_bytes()).unwrap(),
            SecretKeyBytes::from_bytes(f2.view_secret.as_bytes()).unwrap(),
        ];
        let map = spend_secrets_map(&intent, &secrets).unwrap();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&f1.owner_account_pk_hex));
        assert!(map.contains_key(&f2.owner_account_pk_hex));

        // length mismatch is a Validation error.
        let err = spend_secrets_map(&intent, &secrets[..1]).unwrap_err();
        assert_eq!(err.code(), "VALIDATION");
    }

    // Two inputs sharing the same owner pk are rejected by spend_secrets_map (the map would
    // otherwise silently collapse to one entry).
    #[test]
    fn duplicate_owner_pk_is_rejected() {
        let f = make_fixture(1000, 100, 101, 102, 103);
        let (_, com, owner) = want_fields(&f.want);
        let input = StealthInputSpec {
            commitment: crate::types::stealth::CommitmentBytes::from_hex(com).unwrap(),
            owner_account_pk: PublicKeyBytes::from_hex(owner).unwrap(),
        };
        let intent = StealthTransferIntent {
            from_account: crate::types::address::ComponentAddressStr::parse(
                tari_template_lib_types::ComponentAddress::new(ObjectKey::from_array([0xaa; ObjectKey::LENGTH]))
                    .to_string(),
            )
            .unwrap(),
            resource_address: ResourceAddressStr::parse(resource().to_string()).unwrap(),
            fee: BoundaryAmount::new(2000),
            inputs: vec![input.clone(), input],
            outputs: vec![],
            revealed_input_amount: 0,
            revealed_output_amount: 0,
            min_epoch: None,
            max_epoch: None,
            dry_run: false,
            pay_fee_from_revealed: false,
        };
        let secrets = vec![
            SecretKeyBytes::from_bytes(f.view_secret.as_bytes()).unwrap(),
            SecretKeyBytes::from_bytes(f.view_secret.as_bytes()).unwrap(),
        ];
        let err = spend_secrets_map(&intent, &secrets).unwrap_err();
        assert_eq!(err.code(), "VALIDATION");
        assert!(err.to_string().contains("duplicate"));
    }
}
