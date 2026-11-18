//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Signing under [`SchemaVersion::V1`], for tests of V1 verification.
//!
//! The network verifies V1 transactions but every signing API signs [`SchemaVersion::CURRENT`], so
//! these helpers are the only way to produce a V1 authorization or seal.

use tari_crypto::ristretto::RistrettoSecretKey;

use crate::{
    AuthorizationContext,
    SchemaVersion,
    Transaction,
    TransactionMessage,
    TransactionSealSignature,
    TransactionSignature,
    TransactionV1,
    UnsealedTransactionV1,
    UnsignedTransactionV1,
};

/// The V1 authorization message. It does not commit to `context.authorizers()`.
pub fn authorization_message(
    context: AuthorizationContext<'_>,
    transaction: &UnsignedTransactionV1,
) -> TransactionMessage {
    TransactionSignature::create_message_for_schema(SchemaVersion::V1, context, transaction)
}

pub fn sign_authorization(
    secret: &RistrettoSecretKey,
    context: AuthorizationContext<'_>,
    transaction: &UnsignedTransactionV1,
) -> TransactionSignature {
    TransactionSignature::sign_message(secret, &authorization_message(context, transaction))
}

/// The V1 seal message.
pub fn seal_message(transaction: &UnsealedTransactionV1) -> TransactionMessage {
    TransactionSealSignature::create_message_for_schema(
        SchemaVersion::V1,
        transaction,
        &transaction.unsigned_transaction().blobs.hashes(),
    )
}

/// Seals `transaction` as a V1 transaction, keeping its authorization list as given.
pub fn seal(transaction: UnsealedTransactionV1, secret: &RistrettoSecretKey) -> Transaction {
    let signature = TransactionSealSignature::sign_message(secret, &seal_message(&transaction));
    Transaction::V1(TransactionV1::new(transaction, signature))
}
