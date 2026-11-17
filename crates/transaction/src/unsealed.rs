//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

use crate::{
    AuthorizationContext,
    IntoSigned,
    SchemaVersion,
    Signable,
    Transaction,
    TransactionSealSignature,
    TransactionSignature,
    UnsealedTransactionV1,
};

#[derive(Debug, Clone, borsh::BorshSerialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum UnsealedTransaction {
    #[n(0)]
    V1(#[n(0)] UnsealedTransactionV1),
    #[n(1)]
    V2(#[n(0)] UnsealedTransactionV1),
}

impl UnsealedTransaction {
    pub const fn schema(&self) -> SchemaVersion {
        match self {
            Self::V1(_) => SchemaVersion::V1,
            Self::V2(_) => SchemaVersion::V2,
        }
    }

    pub fn schema_version(&self) -> u16 {
        self.schema().as_u16()
    }

    pub fn body(&self) -> &UnsealedTransactionV1 {
        match self {
            Self::V1(t) | Self::V2(t) => t,
        }
    }

    pub fn into_body(self) -> UnsealedTransactionV1 {
        match self {
            Self::V1(t) | Self::V2(t) => t,
        }
    }

    /// Seals the transaction as a [`SchemaVersion::CURRENT`] transaction whatever schema `self` had.
    pub fn seal(self, secret: &RistrettoSecretKey) -> Transaction {
        self.into_body().seal(secret)
    }

    /// Seals the transaction as a [`SchemaVersion::CURRENT`] transaction with a seal signature made
    /// elsewhere over [`TransactionSealSignature::create_message`].
    pub fn seal_with_signature(self, signature: TransactionSealSignature) -> Transaction {
        self.into_body().seal_with_signature(signature)
    }

    /// Replaces the authorization list with one signature per key in `secrets`, each signed under
    /// `seal_signer` and the set of their public keys. The result is a [`SchemaVersion::CURRENT`]
    /// transaction whatever schema `self` had.
    pub fn authorize<'a, I>(self, seal_signer: &RistrettoPublicKeyBytes, secrets: I) -> Self
    where I: IntoIterator<Item = &'a RistrettoSecretKey> {
        let (unsigned, _) = self.into_body().into_parts();
        UnsealedTransactionV1::authorize(unsigned, seal_signer, secrets).into()
    }

    pub fn signatures(&self) -> &[TransactionSignature] {
        match self {
            Self::V1(t) | Self::V2(t) => t.signatures(),
        }
    }

    pub fn is_dry_run(&self) -> bool {
        match self {
            Self::V1(t) | Self::V2(t) => t.is_dry_run(),
        }
    }

    #[must_use]
    pub fn verify_all_signatures(&self, seal_signer: &RistrettoPublicKeyBytes) -> bool {
        let body = self.body();
        body.verify_all_signatures_with_blob_hashes(
            self.schema(),
            seal_signer,
            &body.unsigned_transaction().blobs.hashes(),
        )
    }
}

/// Wraps the body as a [`SchemaVersion::CURRENT`] transaction.
impl From<UnsealedTransactionV1> for UnsealedTransaction {
    fn from(value: UnsealedTransactionV1) -> Self {
        Self::V2(value)
    }
}

impl Signable for UnsealedTransaction {
    type Signature = TransactionSealSignature;

    fn to_signing_message(&self, _context: ()) -> crate::TransactionMessage {
        TransactionSealSignature::create_message(self.body())
    }
}

impl Signable<AuthorizationContext<'_>> for UnsealedTransaction {
    type Signature = TransactionSignature;

    fn to_signing_message(&self, context: AuthorizationContext<'_>) -> crate::TransactionMessage {
        TransactionSignature::create_message(context, self.body().unsigned_transaction())
    }
}

impl IntoSigned for UnsealedTransaction {
    type SignedOutput = Transaction;

    fn into_signed(self, sig: TransactionSealSignature) -> Self::SignedOutput {
        self.seal_with_signature(sig)
    }
}
