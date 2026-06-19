//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Sign → seal → BOR-encode an [`UnsignedTransaction`] into submit-ready bytes + transaction id.
//!
//! Two seal paths are exposed, both consuming an [`UnsignedTransaction`] from [`crate::builder`]:
//!
//! - [`seal_public_transfer`] — the **production** path. It uses the stock random-nonce [`UnsealedTransaction::seal`],
//!   so the bytes and id are unique per call (this is fine — and desirable — for real submission).
//! - [`seal_public_transfer_deterministic`] — the **seed-reproducible** path. It expands the bundle's build seed into
//!   the pinned authorization + seal nonces and threads them through [`RistrettoSchnorr::sign_with_nonce_and_message`],
//!   reconstructing both signatures. With identical inputs it yields byte-identical encoded bytes **and** an identical
//!   transaction id (the id chains the signatures).
//!
//! ### Why determinism needs pinned nonces
//!
//! The stock seal (`UnsealedTransaction::seal` / `TransactionBuilder::build_and_seal`) signs with a
//! **random** Schnorr nonce (`RistrettoSchnorr::sign(secret, msg, &mut rand::rng())` inside
//! `TransactionSignature::sign_v1` and `TransactionSealSignature::sign_v1`). The signing *messages*
//! (`create_message_v1`) are deterministic, but the signatures are not — and `calculate_id` hashes
//! both signatures and the seal signature, so the id moves too. Pinning the nonces is the only way
//! to get reproducible bytes or id.
//!
//! ### The `is_seal_signer_authorized` rule
//!
//! The stock `seal()` sets `is_seal_signer_authorized = true` iff there are no authorization
//! signatures, before it computes the seal message, and that field is committed to by the seal
//! digest. The deterministic path bypasses `seal()`, so it must apply the same rule to the unsigned
//! transaction before computing any message — otherwise the bytes diverge from the stock single-key
//! output.

use ootle_byte_type::ToByteType;
use tari_crypto::ristretto::{RistrettoSchnorr, RistrettoSecretKey};
use tari_ootle_transaction::{
    Transaction,
    TransactionEnvelope,
    TransactionId,
    TransactionSealSignature,
    TransactionSignature,
    UnsealedTransactionV1,
    UnsignedTransaction,
    UnsignedTransactionV1,
};
use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};

use crate::{
    keys::{
        DeterministicTransferKeys,
        PublicTransferKeys,
        parse_nonce_secret,
        parse_secret_key,
        public_key_bytes_from_secret,
    },
    types::error::OotleSdkError,
};

/// Seals an [`UnsignedTransaction`] with the **production** (random-nonce) path.
///
/// Single-key transfer: the account key both authorizes and seals. Returns the sealed
/// [`Transaction`] and its [`TransactionId`]. The bytes/id are **not** reproducible (random nonces).
pub fn seal_public_transfer(
    unsigned: UnsignedTransaction,
    keys: &PublicTransferKeys,
) -> Result<(Transaction, TransactionId), OotleSdkError> {
    let account_secret = parse_secret_key(&keys.account_secret)?;
    let seal_signer_pk = public_key_bytes_from_secret(&account_secret);

    // Authorization signature (stock random nonce), then seal (stock random nonce).
    let unsealed = unsigned.add_signer(&seal_signer_pk, &account_secret);
    let transaction = unsealed.seal(&account_secret);
    let id = transaction.calculate_id();
    Ok((transaction, id))
}

/// Seals an [`UnsignedTransaction`] **deterministically** with the authorization + seal nonces
/// expanded from the bundle's build seed. With the same seed + inputs this is byte-for-byte
/// reproducible (bytes **and** id). A zero seed ⇒ [`OotleSdkError::Validation`].
///
/// For the single-key path (`keys.seal_secret == None`) the account key is both the authorization
/// signer and the seal signer. A separate seal key is supported when `keys.seal_secret` is `Some`.
pub fn seal_public_transfer_deterministic(
    unsigned: UnsignedTransaction,
    keys: &DeterministicTransferKeys,
) -> Result<(Transaction, TransactionId), OotleSdkError> {
    keys.seed.validate_nonzero()?;
    let account_secret = parse_secret_key(&keys.account_secret)?;
    let (auth_nonce_bytes, seal_nonce_bytes) = crate::seed::derive_transfer_nonces(&keys.seed);
    let auth_nonce = parse_nonce_secret(&auth_nonce_bytes)?;
    let seal_nonce = parse_nonce_secret(&seal_nonce_bytes)?;

    // The seal signer is either a separate key or the account key (single-key path).
    let seal_secret = match &keys.seal_secret {
        Some(s) => parse_secret_key(s)?,
        None => account_secret.clone(),
    };
    let seal_signer_pk = public_key_bytes_from_secret(&seal_secret);
    let auth_signer_pk = public_key_bytes_from_secret(&account_secret);

    // Decompose the unsigned transaction into its V1 inner so we can apply the stock `seal()`
    // `is_seal_signer_authorized` rule before computing the seal message.
    let UnsignedTransaction::V1(mut unsigned_v1) = unsigned;

    // 1. The authorization signature(s) this path produces. Computed over the unsigned tx before the flag flip below —
    //    matching the stock flow, where the auth signature is made first (the flag flip only runs when there are zero
    //    auth signatures, so it never changes the message an auth signature was made over).
    let auth_signatures = vec![build_auth_signature(
        &account_secret,
        auth_nonce,
        auth_signer_pk,
        &seal_signer_pk,
        &unsigned_v1,
    )?];

    // 2. The stock `seal()` sets `is_seal_signer_authorized = true` before computing the seal message iff there are
    //    zero authorization signatures, and that field is committed to by the seal digest. We bypass `seal()`, so we
    //    apply the identical rule against the authorization signatures we actually produced — so the bytes match
    //    `seal()` for any count (this path produces one, so the flag keeps its builder default `true`).
    if auth_signatures.is_empty() {
        unsigned_v1.is_seal_signer_authorized = true;
    }

    // 3. Build the unsealed transaction carrying the authorization signature(s).
    let unsealed = UnsealedTransactionV1::new(unsigned_v1, auth_signatures);

    // 4. Seal signature over the deterministic seal message (commits to the prior signatures and to
    //    `is_seal_signer_authorized`), with the pinned seal nonce.
    let seal_message = TransactionSealSignature::create_message_v1(&unsealed);
    let seal_sig_bytes = sign_with_pinned_nonce(&seal_secret, seal_nonce, &seal_message)?;
    let seal_sig = TransactionSealSignature::new(seal_signer_pk, seal_sig_bytes);

    // 5. Inject the seal signature → sealed Transaction; compute the id after sealing.
    let transaction = unsealed.seal_with_signature(seal_sig);
    let id = transaction.calculate_id();
    Ok((transaction, id))
}

/// Seals an [`UnsignedTransaction`] with **externally-supplied authorization signatures** (the
/// co-sign path), using the **production** (random-nonce) seal. The seal signer is derived from
/// `keys.account_secret`; the supplied `auth_signatures` are attached verbatim as the authorization
/// signatures (they were produced by other parties over the auth message committing to this seal
/// signer).
///
/// `is_seal_signer_authorized` is set to `true` iff there are zero authorization signatures,
/// mirroring the stock `seal()`, before the seal message is computed. With auth signatures present
/// the flag stays `false` (the builder default), or the bytes would diverge / the engine would
/// reject.
pub fn seal_public_transfer_with_auth(
    unsigned: UnsignedTransaction,
    keys: &PublicTransferKeys,
    auth_signatures: Vec<TransactionSignature>,
) -> Result<(Transaction, TransactionId), OotleSdkError> {
    let seal_secret = parse_secret_key(&keys.account_secret)?;
    let UnsignedTransaction::V1(mut unsigned_v1) = unsigned;

    // With authorizations present the seal signer is not implicitly authorized (the flag stays
    // `false`); with zero authorizations the stock `seal()` rule sets it `true`. Set it explicitly for
    // both cases so the flag the seal commits to is the same one the supplied authorizations signed
    // over (the co-sign record disables it before A ships it to B).
    unsigned_v1.is_seal_signer_authorized = auth_signatures.is_empty();

    let unsealed = UnsealedTransactionV1::new(unsigned_v1, auth_signatures);
    // Stock random-nonce seal — the bytes/id are intentionally non-reproducible.
    let transaction = unsealed.seal(&seal_secret);
    let id = transaction.calculate_id();
    Ok((transaction, id))
}

/// Deterministic counterpart of [`seal_public_transfer_with_auth`]: seals with the **pinned** seal
/// nonce so the bytes/id are byte-for-byte reproducible. The supplied `auth_signatures` are attached
/// verbatim; the seal signer is `keys.seal_secret` if present, else `keys.account_secret`.
///
/// The seed's authorization nonce is **unused** on this path: this function does not produce its own
/// authorization signature (the authorizations are supplied), only the seal signature.
///
/// Identical `is_seal_signer_authorized` rule as the random twin.
pub fn seal_public_transfer_with_auth_deterministic(
    unsigned: UnsignedTransaction,
    keys: &DeterministicTransferKeys,
    auth_signatures: Vec<TransactionSignature>,
) -> Result<(Transaction, TransactionId), OotleSdkError> {
    keys.seed.validate_nonzero()?;
    let account_secret = parse_secret_key(&keys.account_secret)?;
    let (_auth_nonce_bytes, seal_nonce_bytes) = crate::seed::derive_transfer_nonces(&keys.seed);
    let seal_nonce = parse_nonce_secret(&seal_nonce_bytes)?;
    let seal_secret = match &keys.seal_secret {
        Some(s) => parse_secret_key(s)?,
        None => account_secret,
    };
    let seal_signer_pk = public_key_bytes_from_secret(&seal_secret);

    let UnsignedTransaction::V1(mut unsigned_v1) = unsigned;

    // Explicit flag — `false` with authorizations present (matching the value the co-sign record
    // disabled before A shipped it), `true` with zero authorizations (the stock single-key rule).
    unsigned_v1.is_seal_signer_authorized = auth_signatures.is_empty();

    let unsealed = UnsealedTransactionV1::new(unsigned_v1, auth_signatures);
    let seal_message = TransactionSealSignature::create_message_v1(&unsealed);
    let seal_sig_bytes = sign_with_pinned_nonce(&seal_secret, seal_nonce, &seal_message)?;
    let seal_sig = TransactionSealSignature::new(seal_signer_pk, seal_sig_bytes);

    let transaction = unsealed.seal_with_signature(seal_sig);
    let id = transaction.calculate_id();
    Ok((transaction, id))
}

/// Re-derives the authorization signing message a co-signer must sign: the digest committing to the
/// seal signer's public key and the unsigned transaction (`TransactionSignature::create_message_v1`).
/// A and B both compute this from the identical unsigned record, so they agree on the message.
pub fn cosign_auth_message(seal_signer_pk: &RistrettoPublicKeyBytes, unsigned: &UnsignedTransaction) -> [u8; 64] {
    let UnsignedTransaction::V1(v1) = unsigned;
    TransactionSignature::create_message_v1(seal_signer_pk, v1)
}

/// Builds one authorization [`TransactionSignature`] over the deterministic auth message with the
/// pinned auth nonce. The auth message commits to the **seal signer's** public key (the `SealSigner`
/// preimage field) — the key that will seal the transaction; for the single-key path that is the
/// same key as `auth_signer_pk`.
fn build_auth_signature(
    account_secret: &RistrettoSecretKey,
    auth_nonce: RistrettoSecretKey,
    auth_signer_pk: RistrettoPublicKeyBytes,
    seal_signer_pk: &RistrettoPublicKeyBytes,
    unsigned_v1: &UnsignedTransactionV1,
) -> Result<TransactionSignature, OotleSdkError> {
    let auth_message = TransactionSignature::create_message_v1(seal_signer_pk, unsigned_v1);
    let auth_sig_bytes = sign_with_pinned_nonce(account_secret, auth_nonce, &auth_message)?;
    Ok(TransactionSignature::new(auth_signer_pk, auth_sig_bytes))
}

/// Signs a 64-byte message with a pinned nonce and converts to the byte-typed signature.
///
/// Deterministic counterpart of the stock `sign_v1`: same domain-separated challenge, but the nonce
/// is supplied rather than sampled.
pub(crate) fn sign_with_pinned_nonce(
    secret: &RistrettoSecretKey,
    nonce: RistrettoSecretKey,
    message: &[u8; 64],
) -> Result<SchnorrSignatureBytes, OotleSdkError> {
    let sig = RistrettoSchnorr::sign_with_nonce_and_message(secret, nonce, message)
        .map_err(|e| OotleSdkError::Key(format!("deterministic signing failed: {e}")))?;
    Ok(sig.to_byte_type())
}

/// BOR-encodes a sealed [`Transaction`] into the raw `TransactionEnvelope` wire bytes.
pub fn bor_encode(transaction: Transaction) -> Result<Vec<u8>, OotleSdkError> {
    let envelope = TransactionEnvelope::encode(transaction)
        .map_err(|e| OotleSdkError::Encoding(format!("BOR encode failed: {e}")))?;
    Ok(envelope.0.into_vec())
}

/// Verifies every signature on a sealed transaction (the seal signature and all authorization
/// signatures). A thin wrapper so tests need not reach into the internal type.
#[cfg(test)]
pub(crate) fn verify_all_signatures(transaction: &Transaction) -> bool {
    transaction.verify_all_signatures()
}
