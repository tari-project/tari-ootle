//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Sign → seal → BOR-encode an [`UnsignedTransaction`] into submit-ready bytes + transaction id.
//!
//! Signing always uses a fresh random nonce (`TRANSACTION_SIGNING_CONTEXT.sign` inside
//! `TransactionSignature::sign` / `TransactionSealSignature::sign`),
//! so the sealed bytes and id are unique per call — the safe default for real submission. Nothing here
//! pins a signature nonce from a seed.
//!
//! ### The `main_signer` rule
//!
//! The field defaults to [`MainSigner::SealSigner`], is only changed by explicit choice, and is
//! committed to by every signing message — `seal()` never mutates it. The
//! [`seal_public_transfer_with_auth`] path (which supplies its own authorizations) settles it
//! explicitly before computing any message: [`MainSigner::SealSigner`] with zero authorization
//! signatures, otherwise [`MainSigner::Authorizer`] for the caller's chosen authorizer or
//! [`MainSigner::None`] without one. The co-signers must have signed over the same value.

use tari_ootle_transaction::{
    AuthorizationContext,
    AuthorizerSet,
    MainSigner,
    Transaction,
    TransactionEnvelope,
    TransactionId,
    TransactionMessage,
    TransactionSignature,
    UnsignedTransaction,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    keys::{PublicTransferKeys, parse_secret_key, public_key_bytes_from_secret},
    types::error::OotleSdkError,
};

/// Seals an [`UnsignedTransaction`] with the account key (random nonce).
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
    let unsealed = unsigned.authorize(&seal_signer_pk, [&account_secret]);
    let transaction = unsealed.seal(&account_secret);
    let id = transaction.calculate_id();
    Ok((transaction, id))
}

/// Seals an [`UnsignedTransaction`] with **externally-supplied authorization signatures** (the co-sign
/// path), using the stock random-nonce seal. The seal signer is derived from `keys.account_secret`;
/// the supplied `auth_signatures` are attached as the authorization signatures in canonical order
/// (they were produced by other parties over the auth message committing to this seal signer and to
/// the set of their keys).
///
/// `main_signer` names the authorizer the transaction acts as (`OwnerRule::OwnedBySigner`,
/// `CallerContext::transaction_signer_public_key`); it must be one of the `auth_signatures` keys, else
/// [`OotleSdkError::Invalid`]. The field is set before the seal message is computed (see the module
/// docs), and must match the value the co-sign record shipped, which is what the authorizations signed
/// over. An authorization that does not verify against the sealed transaction, such as one signed over
/// a record declaring a different main signer, ⇒ [`OotleSdkError::Validation`].
pub fn seal_public_transfer_with_auth(
    unsigned: UnsignedTransaction,
    keys: &PublicTransferKeys,
    auth_signatures: Vec<TransactionSignature>,
    main_signer: Option<RistrettoPublicKeyBytes>,
) -> Result<(Transaction, TransactionId), OotleSdkError> {
    let seal_secret = parse_secret_key(&keys.account_secret)?;
    let mut unsigned = unsigned;
    unsigned.set_main_signer(main_signer_with_auth(main_signer, &auth_signatures)?);

    // Stock random-nonce seal — the bytes/id are intentionally non-reproducible.
    let transaction = unsigned.with_signatures(auth_signatures).seal(&seal_secret);
    if !transaction.verify_all_signatures() {
        return Err(OotleSdkError::Validation(
            "the authorizations do not verify against the sealed transaction: each co-signer must sign the record \
             sealed here, including the same main signer"
                .to_string(),
        ));
    }
    let id = transaction.calculate_id();
    Ok((transaction, id))
}

/// The [`MainSigner`] a transaction sealed with `auth_signatures` declares. Without authorizations the
/// seal signer is the only authority, so it is the main signer and no authorizer can be named.
fn main_signer_with_auth(
    main_signer: Option<RistrettoPublicKeyBytes>,
    auth_signatures: &[TransactionSignature],
) -> Result<MainSigner, OotleSdkError> {
    match main_signer {
        None if auth_signatures.is_empty() => Ok(MainSigner::SealSigner),
        None => Ok(MainSigner::None),
        Some(public_key) if auth_signatures.iter().any(|sig| *sig.public_key() == public_key) => {
            Ok(MainSigner::Authorizer(public_key))
        },
        Some(public_key) => Err(OotleSdkError::Invalid(format!(
            "main signer {public_key} must be one of the authorizations, but none is signed by that key"
        ))),
    }
}

/// Re-derives the authorization signing message a co-signer must sign: the digest committing to the
/// unsigned transaction, the authorizer set and the seal signer's public key
/// (`TransactionSignature::create_message`). A and B both compute this from the identical unsigned
/// record, so they agree on the message.
pub fn cosign_auth_message(
    seal_signer_pk: &RistrettoPublicKeyBytes,
    authorizers: &AuthorizerSet,
    unsigned: &UnsignedTransaction,
) -> TransactionMessage {
    TransactionSignature::create_message(AuthorizationContext::new(seal_signer_pk, authorizers), unsigned.body())
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
#[must_use]
pub(crate) fn verify_all_signatures(transaction: &Transaction) -> bool {
    transaction.verify_all_signatures()
}
