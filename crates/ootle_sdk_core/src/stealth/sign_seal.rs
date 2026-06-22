//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Stealth signing + sealing + encoding — the send-side capstone. A pure, synchronous stage driven
//! by the [`StealthSignatureRequirementsState`] the [`StealthPartialTransaction`] carries. The three
//! seal cases are:
//!
//! 1. **Account-key seal** (`must_sign_with_account_key == true`): seal with the caller-supplied account secret; emit
//!    one stealth-key (`c+k`) authorization signature per `other_signers` entry.
//! 2. **Stealth seal** (`!must_sign_with_account_key && seal_signer.is_some()`, or the first input promoted to seal
//!    signer): seal with the one-time key `c+k` derived from the seal input's `public_nonce` via
//!    [`owner_stealth_dh_secret`](tari_ootle_wallet_crypto::kdfs::owner_stealth_dh_secret).
//! 3. **Ephemeral seal** (`can_sign_with_ephemeral_key()`): seal with a one-time ephemeral key — expanded into
//!    [`StealthEntropy::ephemeral_seal_nonce`] (used as the ephemeral *secret*) from the supplied seed on the
//!    seed-reproducible path, or from a fresh OS-RNG seed on the random-nonce default path.
//!
//! Each signature uses the deterministic signing leaf [`crate::tx::sign_with_pinned_nonce`] over the
//! `create_message_v1` digests, and [`crate::tx::bor_encode`] encodes the result. The
//! `is_seal_signer_authorized` flag is set `true` before building the unsealed tx iff there are zero
//! authorization signatures.
//!
//! ## Comparison mode
//!
//! The aggregated bulletproof **and** the balance-proof signature are not byte-stable (their nonces are
//! drawn inside the reused crypto leaves, with no injectable seam). The seal/authorization signatures
//! *this module* produces **are** byte-stable on the deterministic path (pinned nonces), but because
//! they sign a digest that commits to the byte-unstable proofs, the final sealed bytes and id move
//! across runs, so they are compared semantically: the sealed transaction is re-decoded, its
//! signatures re-verified, and its deterministic decoded fields compared, rather than asserting byte
//! equality.
//!
//! ## Auth-nonce derivation (positional contract)
//!
//! [`StealthEntropy`] carries a single `ephemeral_auth_nonce`; a transfer may have *several*
//! `other_signers` that each need a distinct authorization nonce. The seal path domain-separates the
//! base auth nonce per signer index so it stays RNG-free and reproducible without a crypto-leaf change.
//! Signer 0 uses the base nonce verbatim, so the common single-signer case is unaffected; index
//! `i > 0` uses `H(base_nonce || i)` reduced to a canonical scalar. The account-key auth signature (case
//! i) uses the auth nonce expanded from [`StealthKeys::seed`].

use ootle_network::Network as InternalNetwork;
use tari_crypto::{
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_ootle_transaction::{
    Transaction,
    TransactionId,
    TransactionSealSignature,
    TransactionSignature,
    UnsealedTransactionV1,
    UnsignedTransaction,
    UnsignedTransactionV1,
};
use tari_ootle_wallet_crypto::kdfs::owner_stealth_dh_secret;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    inputs::{FetchedSubstate, StealthSignerEntry},
    keys::public_key_bytes_from_secret,
    public_transfer::EncodedPublicTransfer,
    stealth::{
        assemble::build_stealth_transfer_unsigned_with_seed,
        partial::{StealthPartialTransaction, StealthSignatureRequirementsState},
    },
    tx::{bor_encode, sign_with_pinned_nonce},
    types::{
        bytes::{BuildSeed, EncodedTransactionBytes, NonceSecretBytes, SecretKeyBytes, TransactionIdBytes},
        error::OotleSdkError,
        network::Network,
        stealth::{StealthEntropy, StealthTransferIntent, random_seed},
    },
};

/// The caller-supplied secret bundle a stealth seal consumes.
///
/// The account secret is required for **all three** seal cases: case (i) seals with it directly;
/// cases (ii)/(iii) derive the `c+k` one-time keys / authorize stealth signers from it. The build seed
/// expands into the account-key authorization + seal nonces the account-key seal (case i) consumes —
/// the stealth/ephemeral seal nonces come from [`StealthEntropy`] (so a host that only ever does a
/// stealth/ephemeral transfer never has those derived nonces read, but the seed is still required so
/// the bundle stays one shape).
#[derive(Debug, Clone)]
pub struct StealthKeys {
    /// Account secret key (seals directly in case i; derives `c+k` in cases ii/iii).
    pub account_secret: SecretKeyBytes,
    /// The 32-byte build seed expanded into the **account-key** authorization + seal nonces (case i only).
    pub seed: BuildSeed,
}

impl StealthKeys {
    /// Builds the bundle from the account secret and the build seed.
    pub fn new(account_secret: SecretKeyBytes, seed: BuildSeed) -> Self {
        Self { account_secret, seed }
    }
}

/// Parses a boundary secret scalar (account secret / ephemeral seal key) into the internal scalar.
fn parse_secret(bytes: &SecretKeyBytes) -> Result<RistrettoSecretKey, OotleSdkError> {
    RistrettoSecretKey::from_canonical_bytes(bytes.as_bytes())
        .map_err(|e| OotleSdkError::Key(format!("invalid secret key: {e}")))
}

/// Parses a boundary nonce secret into the internal scalar.
fn parse_nonce(bytes: &NonceSecretBytes) -> Result<RistrettoSecretKey, OotleSdkError> {
    RistrettoSecretKey::from_canonical_bytes(bytes.as_bytes())
        .map_err(|e| OotleSdkError::Key(format!("invalid nonce secret: {e}")))
}

/// Recovers the internal `RistrettoPublicKey` for a signer's recorded sender public nonce.
fn parse_signer_public_nonce(signer: &StealthSignerEntry) -> Result<RistrettoPublicKey, OotleSdkError> {
    RistrettoPublicKey::from_canonical_bytes(signer.public_nonce_bytes.as_bytes())
        .map_err(|e| OotleSdkError::Key(format!("invalid signer public nonce: {e}")))
}

/// Derives a signer's one-time spend secret `c+k` from the account secret and the signer's sender
/// public nonce, via the reused KDF leaf. `network` is always the transfer's network — different
/// networks yield different stealth keys.
fn derive_stealth_secret(
    network: InternalNetwork,
    account_secret: &RistrettoSecretKey,
    signer: &StealthSignerEntry,
) -> Result<RistrettoSecretKey, OotleSdkError> {
    let public_nonce = parse_signer_public_nonce(signer)?;
    Ok(owner_stealth_dh_secret(network, account_secret, &public_nonce))
}

/// Derives the pinned authorization nonce for signer index `i` from the base ephemeral auth nonce.
///
/// Index 0 returns the base nonce verbatim (the common single-signer case is byte-identical to a plain
/// pinned nonce); index `i > 0` domain-separates via `from_uniform_bytes(H(base || i))` so each signer
/// gets a distinct, deterministic, canonical scalar without touching an RNG. See the module docs.
fn derive_auth_nonce(base: &RistrettoSecretKey, i: usize) -> RistrettoSecretKey {
    if i == 0 {
        return base.clone();
    }
    use blake2::{Blake2b, Digest, digest::consts::U64};
    use tari_crypto::keys::SecretKey;
    let mut hasher = Blake2b::<U64>::new();
    hasher.update(b"ootle.stealth.auth_nonce.v1");
    hasher.update(base.as_bytes());
    hasher.update((i as u64).to_le_bytes());
    let wide: [u8; 64] = hasher.finalize().into();
    RistrettoSecretKey::from_uniform_bytes(&wide).expect("64 uniform bytes reduce to a canonical scalar")
}

/// Builds one stealth-key (`c+k`) authorization [`TransactionSignature`] for `signer`.
///
/// The message commits to the **seal signer's** public key (`create_message_v1`'s `SealSigner`
/// field); the signature is made with the signer's derived `c+k` secret and pinned nonce; the
/// verifying key recorded is `(c+k)·G`.
fn build_stealth_auth_signature(
    network: InternalNetwork,
    account_secret: &RistrettoSecretKey,
    signer: &StealthSignerEntry,
    auth_nonce: RistrettoSecretKey,
    seal_signer_pk: &RistrettoPublicKeyBytes,
    unsigned_v1: &UnsignedTransactionV1,
) -> Result<TransactionSignature, OotleSdkError> {
    let stealth_secret = derive_stealth_secret(network, account_secret, signer)?;
    let stealth_pk = public_key_bytes_from_secret(&stealth_secret);
    let message = TransactionSignature::create_message_v1(seal_signer_pk, unsigned_v1);
    let sig_bytes = sign_with_pinned_nonce(&stealth_secret, auth_nonce, &message)?;
    Ok(TransactionSignature::new(stealth_pk, sig_bytes))
}

/// The seal key + its pinned nonce selected for a given signature-requirements case. The auth
/// signatures are computed separately (each case decides its own set).
struct SealPlan {
    /// The secret that seals the transaction.
    seal_secret: RistrettoSecretKey,
    /// The pinned nonce for the seal signature.
    seal_nonce: RistrettoSecretKey,
    /// The authorization signatures to attach (in order) before sealing.
    auth_signatures: Vec<TransactionSignature>,
}

/// The account-key nonces case (i) consumes: the seal nonce and the account-key authorization nonce.
/// On the seed-reproducible path these are expanded from [`StealthKeys::seed`]; on the random-nonce
/// default path they are expanded from a fresh OS-RNG seed (so a reused `StealthKeys` bundle never
/// reuses a Schnorr nonce — nonce reuse across two messages with the same key leaks the secret).
struct AccountKeyNonces {
    seal_nonce: RistrettoSecretKey,
    auth_nonce: RistrettoSecretKey,
}

/// Resolves the seal key, seal nonce, and authorization signatures for the given case.
///
/// Every nonce/key is supplied by the caller (no RNG here): `account_key_nonces` carries the case-(i)
/// account-key seal/auth nonces, `entropy` carries the case-(ii)/(iii) stealth/ephemeral nonces +
/// the base auth nonce for stealth-key authorizations. The deterministic entry point pins all of
/// these; the production entry point fills them from `OsRng` before calling.
fn plan_seal(
    network: InternalNetwork,
    account_secret: &RistrettoSecretKey,
    sig_reqs: &StealthSignatureRequirementsState,
    account_key_nonces: AccountKeyNonces,
    entropy: &StealthEntropy,
    unsigned_v1: &UnsignedTransactionV1,
) -> Result<SealPlan, OotleSdkError> {
    // The entropy nonce fields are `SecretKeyBytes` (a 32-byte canonical scalar — the same shape as a
    // nonce secret, just stored in the entropy bundle's uniform secret newtype).
    let base_auth_nonce = parse_secret(&entropy.ephemeral_auth_nonce)?;

    if sig_reqs.must_sign_with_account_key {
        // Case (i): account key seals; account-key auth sig + stealth-key auth sigs for other signers.
        let seal_pk = public_key_bytes_from_secret(account_secret);
        let mut auth_signatures = Vec::with_capacity(sig_reqs.other_signers.len() + 1);

        // The account-key authorization signature.
        let account_auth_msg = TransactionSignature::create_message_v1(&seal_pk, unsigned_v1);
        let account_auth_sig =
            sign_with_pinned_nonce(account_secret, account_key_nonces.auth_nonce, &account_auth_msg)?;
        auth_signatures.push(TransactionSignature::new(seal_pk, account_auth_sig));

        // One stealth-key authorization signature per other signer.
        for (i, signer) in sig_reqs.other_signers_iter().enumerate() {
            auth_signatures.push(build_stealth_auth_signature(
                network,
                account_secret,
                signer,
                derive_auth_nonce(&base_auth_nonce, i),
                &seal_pk,
                unsigned_v1,
            )?);
        }

        Ok(SealPlan {
            seal_secret: account_secret.clone(),
            seal_nonce: account_key_nonces.seal_nonce,
            auth_signatures,
        })
    } else if let Some(seal_signer) = effective_seal_signer(sig_reqs) {
        // Case (ii): a stealth input seals with its one-time key c+k.
        let seal_secret = derive_stealth_secret(network, account_secret, seal_signer)?;
        let seal_pk = public_key_bytes_from_secret(&seal_secret);

        // Authorization signatures for the remaining required signers (the seal signer is skipped by
        // `other_signers_iter` when it was the promoted first input).
        let mut auth_signatures = Vec::new();
        for (i, signer) in sig_reqs.other_signers_iter().enumerate() {
            auth_signatures.push(build_stealth_auth_signature(
                network,
                account_secret,
                signer,
                derive_auth_nonce(&base_auth_nonce, i),
                &seal_pk,
                unsigned_v1,
            )?);
        }

        Ok(SealPlan {
            seal_secret,
            seal_nonce: parse_secret(&entropy.ephemeral_sign_nonce)?,
            auth_signatures,
        })
    } else {
        // Case (iii): ephemeral key seal (no inputs to spend). No authorization signatures.
        let seal_secret = parse_secret(&entropy.ephemeral_seal_nonce)?;
        Ok(SealPlan {
            seal_secret,
            seal_nonce: parse_secret(&entropy.ephemeral_sign_nonce)?,
            auth_signatures: Vec::new(),
        })
    }
}

/// The seal signer for the stealth-seal case: the explicit `seal_signer`, else the first required
/// signer promoted to seal signer. Returns `None` for the account-key case (the account key seals,
/// not a stealth signer) and for the ephemeral case (no signers at all). The
/// `must_sign_with_account_key` short-circuit makes the case-(i) invariant self-enforcing even if a
/// future caller invokes this before the dispatch guard.
fn effective_seal_signer(sig_reqs: &StealthSignatureRequirementsState) -> Option<&StealthSignerEntry> {
    if sig_reqs.must_sign_with_account_key {
        return None;
    }
    sig_reqs.seal_signer.as_ref().or_else(|| sig_reqs.other_signers.first())
}

/// Seals + encodes an already-assembled [`StealthPartialTransaction`] **seed-reproducibly** (every
/// nonce/key expanded from `keys.seed` + `seed`). Returns submit-ready BOR bytes + the transaction id.
///
/// `keys.seed` expands into the account-key (case i) authorization + seal nonces; `seed` expands into
/// the stealth/ephemeral seal nonces (cases ii/iii) and the stealth-key auth base. The seal case is
/// driven by the partial's [`StealthSignatureRequirementsState`]. The bytes are byte-stable *for the
/// signatures this function produces*, but the embedded proofs are not, so the full send vectors
/// compare semantically.
pub fn seal_and_encode_stealth_transfer_with_seed(
    network: Network,
    partial: StealthPartialTransaction,
    keys: &StealthKeys,
    seed: &BuildSeed,
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    keys.seed.validate_nonzero()?;
    let internal_network: InternalNetwork = network.into();
    let account_secret = parse_secret(&keys.account_secret)?;

    let (unsigned, sig_reqs) = partial.into_parts();
    let UnsignedTransaction::V1(mut unsigned_v1) = unsigned;

    // The seal-side entropy (case-ii/iii stealth/ephemeral nonces + the stealth-key auth base). `0`
    // outputs is enough — the seal-side fields are always present (the per-output proof entropy is
    // unused here; the proofs were already built).
    let entropy = StealthEntropy::from_seed(seed, 0);

    // The account-key (case i) seal/auth nonces, expanded from the keys bundle's own seed.
    let (auth_nonce_bytes, seal_nonce_bytes) = crate::seed::derive_transfer_nonces(&keys.seed);
    let account_key_nonces = AccountKeyNonces {
        seal_nonce: parse_nonce(&seal_nonce_bytes)?,
        auth_nonce: parse_nonce(&auth_nonce_bytes)?,
    };

    let plan = plan_seal(
        internal_network,
        &account_secret,
        &sig_reqs,
        account_key_nonces,
        &entropy,
        &unsigned_v1,
    )?;

    let transaction = finalize_seal(&mut unsigned_v1, plan)?;
    let id = transaction.calculate_id();
    encode(transaction, id)
}

/// Seals + encodes an already-assembled [`StealthPartialTransaction`] on the **random-nonce default**
/// path: every signature nonce (the account-key seal/auth nonces, the stealth-key auth nonces, the
/// stealth/ephemeral seal nonces, and the ephemeral seal *key*) is expanded from a fresh OS-RNG seed,
/// so the output is **not** reproducible (the safe default for real submission) and, crucially, a
/// reused `StealthKeys` bundle never reuses a Schnorr nonce.
///
/// `keys.account_secret` is still consumed (it seals case i and derives the case-ii/iii keys); the
/// `keys.seed` field is **ignored** on this path — the nonces come from the freshly-drawn seed below.
pub fn seal_and_encode_stealth_transfer(
    network: Network,
    partial: StealthPartialTransaction,
    keys: &StealthKeys,
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let internal_network: InternalNetwork = network.into();
    let account_secret = parse_secret(&keys.account_secret)?;

    let (unsigned, sig_reqs) = partial.into_parts();
    let UnsignedTransaction::V1(mut unsigned_v1) = unsigned;

    // A freshly-drawn entropy bundle supplies the case-(ii)/(iii) stealth/ephemeral seal nonces +
    // ephemeral key + the base stealth-key auth nonce. `0` outputs is enough — the seal-side fields
    // are always present (the per-output proof entropy is unused here; the proofs were already built).
    let entropy = StealthEntropy::from_os_rng(0);
    // Fresh case-(i) account-key seal/auth nonces (NOT from `keys` — avoids Schnorr nonce reuse). Each
    // maps to a distinct independent OsRng-drawn field so no two signatures share a nonce: the account
    // seal uses `ephemeral_seal_nonce`, the account auth uses `ephemeral_sign_nonce`, and the
    // stealth-key auth base inside `plan_seal` uses `ephemeral_auth_nonce`.
    let account_key_nonces = AccountKeyNonces {
        seal_nonce: parse_secret(&entropy.ephemeral_seal_nonce)?,
        auth_nonce: parse_secret(&entropy.ephemeral_sign_nonce)?,
    };

    let plan = plan_seal(
        internal_network,
        &account_secret,
        &sig_reqs,
        account_key_nonces,
        &entropy,
        &unsigned_v1,
    )?;

    let transaction = finalize_seal(&mut unsigned_v1, plan)?;
    let id = transaction.calculate_id();
    encode(transaction, id)
}

/// Sets `is_seal_signer_authorized`, builds the unsealed tx with the planned authorization signatures,
/// computes the seal signature over the seal digest with the planned key+nonce, and injects it.
fn finalize_seal(unsigned_v1: &mut UnsignedTransactionV1, plan: SealPlan) -> Result<Transaction, OotleSdkError> {
    // The stock `seal()` sets `is_seal_signer_authorized = true` BEFORE computing the seal message iff
    // there are zero authorization signatures. The flag is committed by the seal digest, so it must be
    // applied before constructing the unsealed tx.
    if plan.auth_signatures.is_empty() {
        unsigned_v1.is_seal_signer_authorized = true;
    }

    let seal_pk = public_key_bytes_from_secret(&plan.seal_secret);
    let unsealed = UnsealedTransactionV1::new(unsigned_v1.clone(), plan.auth_signatures);

    let seal_message = TransactionSealSignature::create_message_v1(&unsealed);
    let seal_sig_bytes = sign_with_pinned_nonce(&plan.seal_secret, plan.seal_nonce, &seal_message)?;
    let seal_sig = TransactionSealSignature::new(seal_pk, seal_sig_bytes);

    Ok(unsealed.seal_with_signature(seal_sig))
}

/// Wraps the sealed transaction + id into the boundary [`EncodedPublicTransfer`] (encoded bytes + id,
/// both lowercase hex).
fn encode(transaction: Transaction, id: TransactionId) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let bytes = bor_encode(transaction)?;
    Ok(EncodedPublicTransfer {
        encoded_transaction: EncodedTransactionBytes::from_vec(bytes),
        transaction_id: TransactionIdBytes::from_bytes(id.as_bytes()),
    })
}

/// Full-pipeline seed-reproducible send: build → sign/seal/encode in one call. `seed` expands into all
/// proof + seal-side entropy; `keys.seed` expands into the account-key (case i) nonces.
///
/// `fetched_utxos` are every stealth-input UTXO substate, fetched up front; `spend_secrets` are the
/// per-input view-only secrets (one per `intent.inputs`, positional) used to decrypt the input masks.
pub fn build_and_encode_stealth_transfer_with_seed(
    network: Network,
    intent: &StealthTransferIntent,
    fetched_utxos: &[FetchedSubstate],
    spend_secrets: &[SecretKeyBytes],
    keys: &StealthKeys,
    seed: &BuildSeed,
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let partial = build_stealth_transfer_unsigned_with_seed(network, intent, fetched_utxos, spend_secrets, seed)?;
    seal_and_encode_stealth_transfer_with_seed(network, partial, keys, seed)
}

/// Full-pipeline random-nonce default send: expands the proof + seal entropy from a fresh OS-RNG seed,
/// then builds + seals. The output is **not** reproducible (the safe default for real submission).
pub fn build_and_encode_stealth_transfer(
    network: Network,
    intent: &StealthTransferIntent,
    fetched_utxos: &[FetchedSubstate],
    spend_secrets: &[SecretKeyBytes],
    keys: &StealthKeys,
) -> Result<EncodedPublicTransfer, OotleSdkError> {
    let seed = random_seed();
    let partial = build_stealth_transfer_unsigned_with_seed(network, intent, fetched_utxos, spend_secrets, &seed)?;
    seal_and_encode_stealth_transfer(network, partial, keys)
}

#[cfg(test)]
mod tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoSecretKey};
    use tari_engine_types::{
        Utxo,
        UtxoOutput,
        crypto::{OutputBody, commit_u64_amount},
        substate::SubstateValue,
    };
    use tari_ootle_transaction::TransactionEnvelope;
    use tari_ootle_wallet_crypto::{encrypted_data::encrypt_data, kdfs};
    use tari_template_lib_types::{
        ComponentAddress,
        ObjectKey,
        access_rules::AccessRule,
        crypto::UtxoTag,
        stealth::SpendCondition,
    };

    use super::*;
    use crate::{
        inputs::FetchedSubstate,
        stealth::inputs::stealth_utxo_substate_id,
        tx::verify_all_signatures,
        types::{
            address::{ComponentAddressStr, ResourceAddressStr},
            bytes::PublicKeyBytes,
            numeric::BoundaryAmount,
            stealth::{StealthInputSpec, StealthOutputSpec, StealthPayTo},
        },
    };

    const NETWORK: Network = Network::LocalNet;

    fn fixed_scalar(seed: u8) -> RistrettoSecretKey {
        let mut b = [0u8; 32];
        b[0] = seed;
        RistrettoSecretKey::from_canonical_bytes(&b).expect("low scalar is canonical")
    }

    fn secret(seed: u8) -> SecretKeyBytes {
        SecretKeyBytes::from_bytes(fixed_scalar(seed).as_bytes()).unwrap()
    }

    /// A fixed build seed for the seed-reproducible seal/build paths.
    fn build_seed(byte: u8) -> BuildSeed {
        BuildSeed::from_array([byte; 32])
    }

    fn pk_bytes(seed: u8) -> PublicKeyBytes {
        let pk = RistrettoPublicKey::from_secret_key(&fixed_scalar(seed));
        PublicKeyBytes::from_bytes(pk.as_bytes()).unwrap()
    }

    fn tari_resource() -> ResourceAddressStr {
        ResourceAddressStr::parse(tari_template_lib_types::constants::STEALTH_TARI_RESOURCE_ADDRESS.to_string())
            .unwrap()
    }

    fn from_component() -> ComponentAddressStr {
        ComponentAddressStr::parse(ComponentAddress::new(ObjectKey::from_array([0xaa; ObjectKey::LENGTH])).to_string())
            .unwrap()
    }

    fn output_spec(amount: u64) -> StealthOutputSpec {
        StealthOutputSpec {
            destination_account_pk: pk_bytes(3),
            destination_view_pk: pk_bytes(4),
            amount,
            revealed_amount: 0,
            resource_address: tari_resource(),
            resource_view_key: None,
            memo: None,
            pay_to: StealthPayTo::StealthPublicKey,
            utxo_tag: None,
            minimum_value_promise: 0,
        }
    }

    fn stealth_keys() -> StealthKeys {
        StealthKeys::new(secret(11), build_seed(0x42))
    }

    /// Builds an ephemeral-case (iii) partial.
    ///
    /// # Why the ephemeral case is cryptographically unreachable in a balanced transfer
    ///
    /// The genuine ephemeral case (`can_sign_with_ephemeral_key()`) requires `must_sign_with_account_key
    /// == false` (⇒ `revealed_input_amount == 0`) **and** `seal_signer.is_none() && other_signers
    /// .is_empty()` (⇒ no stealth inputs). That shape — `inputs_statement.revealed_amount == 0` **and**
    /// `inputs_statement.inputs.is_empty()` — is **categorically rejected** by the engine's
    /// `validate_transfer` pre-flight (called before emitting any partial):
    ///
    /// ```text
    /// if transfer.inputs_statement.revealed_amount.is_zero() && transfer.inputs_statement.inputs.is_empty() {
    ///     return Err(InvalidBalanceProof { details: "No inputs or revealed inputs provided" });
    /// }
    /// ```
    ///
    /// This is a **cryptographic invariant, not an accidental gap**: with no stealth inputs and no
    /// stealth outputs the public excess is `0·G`, so the balance signature `r + e·0 = r` would leak the
    /// signing nonce. There is therefore **no balanced transaction shape that triggers the ephemeral case
    /// yet passes assembly**, so the ephemeral seal is covered only by the unit tests below.
    ///
    /// ## How this helper exercises the path
    ///
    /// We take a valid, fully-assembled unsigned transaction (the account-key intent, which assembles +
    /// validates) and pair it with a hand-built ephemeral `StealthSignatureRequirementsState`.
    /// `seal_and_encode` only consumes the sig-reqs + the unsigned tx (it does not re-validate), so this
    /// faithfully exercises the case-(iii) seal path against a real, balanced transaction body.
    fn ephemeral_partial() -> StealthPartialTransaction {
        let (unsigned, _sig_reqs) = account_key_partial().into_parts();
        let ephemeral_reqs = StealthSignatureRequirementsState {
            must_sign_with_account_key: false,
            seal_signer: None,
            other_signers: Vec::new(),
        };
        assert!(
            ephemeral_reqs.can_sign_with_ephemeral_key(),
            "the hand-built sig-reqs must be the ephemeral case"
        );
        StealthPartialTransaction {
            unsigned,
            sig_reqs: ephemeral_reqs,
        }
    }

    /// The build seed the stealth send tests expand into proof + seal-side entropy.
    fn send_seed() -> BuildSeed {
        build_seed(0x90)
    }

    /// Builds a stealth-seal (case ii) partial from a fabricated, decryptable stealth UTXO input
    /// balanced against an equal-value output. Returns the partial + the (view_secret, public_nonce,
    /// owner_account_secret) so the test can reconstruct the expected c+k seal key.
    fn stealth_seal_partial() -> (
        StealthPartialTransaction,
        RistrettoSecretKey,
        RistrettoPublicKey,
        BuildSeed,
    ) {
        let value = 1_000_000u64;
        let seed = send_seed();

        let input_mask = fixed_scalar(160);
        let nonce_secret = fixed_scalar(161);
        let public_nonce = RistrettoPublicKey::from_secret_key(&nonce_secret);
        let view_secret = fixed_scalar(162);

        let commitment = commit_u64_amount(&input_mask, value).to_byte_type();
        let commitment_hex = hex::encode(commitment.as_bytes());

        let encryption_key = kdfs::encrypted_data_dh_kdf_aead(&view_secret, &public_nonce);
        let encrypted_data = encrypt_data(value, &input_mask, &encryption_key, None).unwrap();
        let utxo = Utxo::new(UtxoOutput {
            output: OutputBody {
                public_nonce: public_nonce.to_byte_type(),
                encrypted_data,
                minimum_value_promise: 0,
                viewable_balance: None,
            },
            spend_condition: SpendCondition::AccessRule(AccessRule::AllowAll),
            tag: UtxoTag::new(0),
        });

        // The owner account secret: we use the test account secret (so c+k = H(net,acct,R)+acct).
        let owner_account_secret = fixed_scalar(11);
        let owner_pk = RistrettoPublicKey::from_secret_key(&owner_account_secret);
        let owner_pk_bytes = PublicKeyBytes::from_bytes(owner_pk.as_bytes()).unwrap();

        let substate_id = stealth_utxo_substate_id(tari_resource().as_str(), &commitment_hex).unwrap();
        let fetched = vec![FetchedSubstate {
            substate_id: substate_id.to_string(),
            version: 0,
            substate_value: serde_json::to_value(SubstateValue::Utxo(utxo)).unwrap(),
        }];

        let intent = StealthTransferIntent {
            from_account: from_component(),
            resource_address: tari_resource(),
            fee: BoundaryAmount::new(2000),
            inputs: vec![StealthInputSpec {
                commitment: crate::types::stealth::CommitmentBytes::from_hex(&commitment_hex).unwrap(),
                owner_account_pk: owner_pk_bytes,
            }],
            outputs: vec![output_spec(value)],
            revealed_input_amount: 0,
            revealed_output_amount: 0,
            min_epoch: None,
            max_epoch: None,
            dry_run: false,
            pay_fee_from_revealed: false,
        };

        let spend_secrets = vec![SecretKeyBytes::from_bytes(view_secret.as_bytes()).unwrap()];
        let partial =
            build_stealth_transfer_unsigned_with_seed(NETWORK, &intent, &fetched, &spend_secrets, &seed).unwrap();
        (partial, owner_account_secret, public_nonce, seed)
    }

    /// Builds an account-key-seal (case i) partial: a stealth output funded by a revealed-input
    /// bucket (`revealed_input_amount > 0` ⇒ the account key must sign).
    fn account_key_partial() -> StealthPartialTransaction {
        let intent = StealthTransferIntent {
            from_account: from_component(),
            resource_address: tari_resource(),
            fee: BoundaryAmount::new(2000),
            inputs: vec![],
            outputs: vec![output_spec(1_000_000)],
            revealed_input_amount: 1_000_000,
            revealed_output_amount: 0,
            min_epoch: None,
            max_epoch: None,
            dry_run: false,
            pay_fee_from_revealed: false,
        };
        build_stealth_transfer_unsigned_with_seed(NETWORK, &intent, &[], &[], &send_seed()).unwrap()
    }

    fn decode(out: &EncodedPublicTransfer) -> Transaction {
        TransactionEnvelope::from_raw(out.encoded_transaction.as_bytes().to_vec().into_boxed_slice())
            .decode()
            .expect("sealed bytes decode")
    }

    // (a) Ephemeral seal — signatures verify, id is 32 bytes, bytes non-empty.
    #[test]
    fn ephemeral_seal_signatures_verify() {
        let seed = send_seed();
        let partial = ephemeral_partial();
        let out = seal_and_encode_stealth_transfer_with_seed(NETWORK, partial, &stealth_keys(), &seed).unwrap();
        assert!(!out.encoded_transaction.as_bytes().is_empty());
        assert_eq!(out.transaction_id.as_bytes().len(), 32);
        let tx = decode(&out);
        assert!(verify_all_signatures(&tx), "ephemeral seal signature must verify");

        // The seal key is the seed-expanded ephemeral key (ephemeral_seal_nonce as secret), NOT the account key.
        let eph_secret = parse_secret(&crate::seed::derive_ephemeral_seal_nonce(&seed)).unwrap();
        let eph_pk = public_key_bytes_from_secret(&eph_secret);
        assert_eq!(tx.seal_signature().public_key(), &eph_pk, "ephemeral key seals");
        let account_pk = public_key_bytes_from_secret(&fixed_scalar(11));
        assert_ne!(tx.seal_signature().public_key(), &account_pk, "not the account key");
    }

    // (b) Ephemeral seal — reproducibility (semantic): identical seed ⇒ identical ephemeral seal key
    //     + verifying signatures across two runs. (The underlying tx body carries a balance proof +
    //     bulletproof whose nonces are drawn in the reused crypto leaves, so the raw bytes move — the
    //     seal-side determinism this module owns is what we assert.)
    #[test]
    fn ephemeral_seal_is_reproducible() {
        let seed = send_seed();
        let a =
            seal_and_encode_stealth_transfer_with_seed(NETWORK, ephemeral_partial(), &stealth_keys(), &seed).unwrap();
        let b =
            seal_and_encode_stealth_transfer_with_seed(NETWORK, ephemeral_partial(), &stealth_keys(), &seed).unwrap();
        let (ta, tb) = (decode(&a), decode(&b));
        assert_eq!(
            ta.seal_signature().public_key(),
            tb.seal_signature().public_key(),
            "the pinned ephemeral seal key is reproducible across runs"
        );
        assert!(verify_all_signatures(&ta) && verify_all_signatures(&tb));
    }

    // (c) Stealth seal (c+k) — signatures verify; the seal public key equals (c+k)·G.
    #[test]
    fn stealth_seal_signatures_verify_and_key_matches() {
        let (partial, account_secret, public_nonce, seed) = stealth_seal_partial();
        assert!(!partial.signature_requirements().must_sign_with_account_key);
        assert!(partial.signature_requirements().seal_signer.is_some());

        let out = seal_and_encode_stealth_transfer_with_seed(NETWORK, partial, &stealth_keys(), &seed).unwrap();
        let tx = decode(&out);
        assert!(verify_all_signatures(&tx), "stealth seal signatures must verify");

        // The expected c+k seal key.
        let internal: InternalNetwork = NETWORK.into();
        let expected_secret = owner_stealth_dh_secret(internal, &account_secret, &public_nonce);
        let expected_pk = public_key_bytes_from_secret(&expected_secret);
        assert_eq!(
            tx.seal_signature().public_key(),
            &expected_pk,
            "seal signer public key must be (c+k)·G"
        );
    }

    // (d) Account-key seal — auth signature + seal signature both verify.
    #[test]
    fn account_key_seal_signatures_verify() {
        let partial = account_key_partial();
        assert!(partial.signature_requirements().must_sign_with_account_key);
        let out = seal_and_encode_stealth_transfer_with_seed(NETWORK, partial, &stealth_keys(), &send_seed()).unwrap();
        let tx = decode(&out);
        assert!(
            verify_all_signatures(&tx),
            "account-key auth + seal signatures must verify"
        );

        // The seal signer is the account key.
        let expected_pk = public_key_bytes_from_secret(&fixed_scalar(11));
        assert_eq!(tx.seal_signature().public_key(), &expected_pk);
    }

    // (e) All three production paths produce non-empty bytes + a 32-byte id.
    #[test]
    fn production_paths_produce_bytes_and_id() {
        // Ephemeral.
        let eph = seal_and_encode_stealth_transfer(NETWORK, ephemeral_partial(), &stealth_keys()).unwrap();
        assert!(!eph.encoded_transaction.as_bytes().is_empty());
        assert_eq!(eph.transaction_id.as_bytes().len(), 32);

        // Stealth seal.
        let (partial, _, _, _) = stealth_seal_partial();
        let st = seal_and_encode_stealth_transfer(NETWORK, partial, &stealth_keys()).unwrap();
        assert!(!st.encoded_transaction.as_bytes().is_empty());
        assert_eq!(st.transaction_id.as_bytes().len(), 32);
        assert!(verify_all_signatures(&decode(&st)));

        // Account key.
        let ak = seal_and_encode_stealth_transfer(NETWORK, account_key_partial(), &stealth_keys()).unwrap();
        assert!(!ak.encoded_transaction.as_bytes().is_empty());
        assert_eq!(ak.transaction_id.as_bytes().len(), 32);
        assert!(verify_all_signatures(&decode(&ak)));
    }

    // (f) is_seal_signer_authorized. Ephemeral (0 auth sigs) ⇒ true. Account-key (1+ auth sigs) ⇒ also
    //     true (builder default; the auth sig covers the seal key per the stock semantics).
    #[test]
    fn is_seal_signer_authorized_flag_set() {
        let eph =
            seal_and_encode_stealth_transfer_with_seed(NETWORK, ephemeral_partial(), &stealth_keys(), &send_seed())
                .unwrap();
        let eph_tx = decode(&eph);
        assert!(
            eph_tx.is_seal_signer_authorized(),
            "ephemeral (0 auth sigs) ⇒ is_seal_signer_authorized must be true"
        );

        let ak =
            seal_and_encode_stealth_transfer_with_seed(NETWORK, account_key_partial(), &stealth_keys(), &send_seed())
                .unwrap();
        assert!(
            decode(&ak).is_seal_signer_authorized(),
            "account-key path ⇒ is_seal_signer_authorized must be true"
        );
    }

    // (g) Stealth-seal bytes are reproducible modulo the non-byte-stable proofs: the seal/auth
    //     signature public keys + the id-over-deterministic-fields are stable. We assert the decoded
    //     seal public key + the verification both hold across two runs (the bytes themselves move
    //     because the embedded balance proof + bulletproof draw fresh nonces in the crypto leaves).
    #[test]
    fn stealth_seal_reproducible_modulo_proofs() {
        let (partial_a, _, _, seed) = stealth_seal_partial();
        let (partial_b, _, _, _) = stealth_seal_partial();
        let a = seal_and_encode_stealth_transfer_with_seed(NETWORK, partial_a, &stealth_keys(), &seed).unwrap();
        let b = seal_and_encode_stealth_transfer_with_seed(NETWORK, partial_b, &stealth_keys(), &seed).unwrap();
        let (ta, tb) = (decode(&a), decode(&b));
        assert_eq!(
            ta.seal_signature().public_key(),
            tb.seal_signature().public_key(),
            "the c+k seal key is deterministic across runs"
        );
        assert!(verify_all_signatures(&ta) && verify_all_signatures(&tb));
    }

    // (h) Malformed account secret ⇒ KEY error, never a panic.
    #[test]
    fn malformed_account_secret_is_key_error() {
        let bad = StealthKeys::new(SecretKeyBytes::from_array([0xff; 32]), build_seed(0x42));
        let err =
            seal_and_encode_stealth_transfer_with_seed(NETWORK, ephemeral_partial(), &bad, &send_seed()).unwrap_err();
        assert_eq!(err.code(), "KEY");
    }
}
