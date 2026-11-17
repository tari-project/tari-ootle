//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use indexmap::IndexSet;
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::{indexed_value::IndexedValueError, substate::SubstateId};
use tari_ootle_common_types::{Epoch, InputDeclaration};
use tari_template_lib_types::{ComponentAddress, crypto::RistrettoPublicKeyBytes};

use crate::{
    AuthorizationContext,
    BlobHashes,
    Blobs,
    Instruction,
    IntoSigned,
    MAX_SIGNATURES_PER_TRANSACTION,
    SchemaVersion,
    Transaction,
    TransactionSealSignature,
    signable::Signable,
    v1::{
        authorizers::{AuthorizerSet, canonicalize_authorization_list, is_canonical_authorization_list},
        signature::TransactionSignature,
        transaction::TransactionV1,
        unsigned::UnsignedTransactionV1,
    },
};

pub(crate) fn decode_signatures<'b, C>(
    d: &mut minicbor::Decoder<'b>,
    ctx: &mut C,
) -> Result<Vec<TransactionSignature>, minicbor::decode::Error> {
    tari_bor::adapters::bounded_vec::decode(d, ctx, MAX_SIGNATURES_PER_TRANSACTION)
}

#[derive(Debug, Clone, borsh::BorshSerialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UnsealedTransactionV1 {
    #[n(0)]
    transaction: UnsignedTransactionV1,
    #[n(1)]
    #[cbor(decode_with = "decode_signatures")]
    signatures: Vec<TransactionSignature>,
}

impl UnsealedTransactionV1 {
    /// Pairs `unsigned_transaction` with `signatures` exactly as given. A list that is not strictly
    /// ascending by public key fails verification; [`Self::with_canonical_signatures`] orders it.
    pub fn new(unsigned_transaction: UnsignedTransactionV1, signatures: Vec<TransactionSignature>) -> Self {
        Self {
            transaction: unsigned_transaction,
            signatures,
        }
    }

    /// Pairs `unsigned_transaction` with `signatures` ordered by public key, keeping the first
    /// signature of a repeated key.
    pub fn with_canonical_signatures(
        unsigned_transaction: UnsignedTransactionV1,
        mut signatures: Vec<TransactionSignature>,
    ) -> Self {
        canonicalize_authorization_list(&mut signatures);
        Self::new(unsigned_transaction, signatures)
    }

    /// Signs `unsigned_transaction` with every key in `secrets` under `seal_signer` and the set of
    /// their public keys. The resulting list holds exactly those authorizations.
    pub fn authorize<'a, I>(
        unsigned_transaction: UnsignedTransactionV1,
        seal_signer: &RistrettoPublicKeyBytes,
        secrets: I,
    ) -> Self
    where
        I: IntoIterator<Item = &'a RistrettoSecretKey>,
    {
        let signatures = sign_authorizations(&unsigned_transaction, seal_signer, secrets);
        Self::new(unsigned_transaction, signatures)
    }

    /// Seals the transaction as a [`SchemaVersion::CURRENT`] transaction.
    pub fn seal(self, secret: &RistrettoSecretKey) -> Transaction {
        let sig = TransactionSealSignature::sign(secret, &self);
        self.set_seal_signature(sig)
    }

    fn set_seal_signature(self, signature: TransactionSealSignature) -> Transaction {
        Transaction::from_body(SchemaVersion::CURRENT, TransactionV1::new(self, signature))
    }

    /// Seals the transaction as a [`SchemaVersion::CURRENT`] transaction with a seal signature made
    /// elsewhere.
    pub fn seal_with_signature(self, signature: TransactionSealSignature) -> Transaction {
        self.set_seal_signature(signature)
    }

    pub fn is_dry_run(&self) -> bool {
        self.transaction.dry_run
    }

    pub fn unsigned_transaction(&self) -> &UnsignedTransactionV1 {
        &self.transaction
    }

    pub fn fee_instructions(&self) -> &[Instruction] {
        &self.transaction.fee_instructions
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.transaction.instructions
    }

    pub fn blobs(&self) -> &Blobs {
        self.transaction.blobs()
    }

    pub fn signatures(&self) -> &[TransactionSignature] {
        &self.signatures
    }

    /// Verifies every authorization signature under `schema`.
    #[must_use]
    pub fn verify_all_signatures(&self, schema: SchemaVersion, seal_signer: &RistrettoPublicKeyBytes) -> bool {
        self.verify_all_signatures_with_blob_hashes(schema, seal_signer, &self.transaction.blobs.hashes())
    }

    /// Verifies every authorization signature against a transaction whose blob commitments have
    /// already been derived.
    ///
    /// Deriving them hashes every blob payload, so a caller that also verifies the seal should
    /// derive [`Blobs::hashes`] once and use this for both.
    #[must_use]
    pub fn verify_all_signatures_with_blob_hashes(
        &self,
        schema: SchemaVersion,
        seal_signer: &RistrettoPublicKeyBytes,
        blob_hashes: &BlobHashes,
    ) -> bool {
        if self.signatures().is_empty() {
            return true;
        }
        if schema.binds_authorizer_set() && !is_canonical_authorization_list(self.signatures()) {
            return false;
        }

        // Derived once and reused: every signature over this transaction signs the same message, and
        // deriving it hashes the whole body including a commitment over every blob's bytes.
        let message = TransactionSignature::create_message_for_list(
            schema,
            seal_signer,
            &self.transaction,
            blob_hashes,
            self.signatures(),
        );
        TransactionSignature::verify_all_against_message(self.signatures(), message)
    }

    /// True when the authorization list is strictly ascending by public key.
    pub fn has_canonical_signatures(&self) -> bool {
        is_canonical_authorization_list(self.signatures())
    }

    pub fn inputs(&self) -> &IndexSet<InputDeclaration> {
        &self.transaction.inputs
    }

    /// Returns (fee instructions, instructions)
    pub fn into_instruction_parts(self) -> (Vec<Instruction>, Vec<Instruction>) {
        (self.transaction.fee_instructions, self.transaction.instructions)
    }

    /// Returns (unsigned, signatures)
    pub fn into_parts(self) -> (UnsignedTransactionV1, Vec<TransactionSignature>) {
        (self.transaction, self.signatures)
    }

    pub fn min_epoch(&self) -> Option<Epoch> {
        self.transaction.min_epoch
    }

    pub fn max_epoch(&self) -> Epoch {
        self.transaction.max_epoch
    }

    pub fn as_referenced_components(&self) -> impl Iterator<Item = &ComponentAddress> + '_ {
        self.transaction.as_referenced_components()
    }

    /// Returns all substates addresses referenced by this transaction
    pub fn to_referenced_substates(&self) -> Result<HashSet<SubstateId>, IndexedValueError> {
        self.transaction.to_referenced_substates()
    }

    pub fn has_inputs_without_version(&self) -> bool {
        self.inputs().iter().any(|i| i.version().is_none())
    }
}

impl Signable for UnsealedTransactionV1 {
    type Signature = TransactionSealSignature;

    fn to_signing_message(&self, _context: ()) -> crate::TransactionMessage {
        TransactionSealSignature::create_message(self)
    }
}

impl Signable<AuthorizationContext<'_>> for UnsealedTransactionV1 {
    type Signature = TransactionSignature;

    fn to_signing_message(&self, context: AuthorizationContext<'_>) -> crate::TransactionMessage {
        TransactionSignature::create_message(context, &self.transaction)
    }
}

impl IntoSigned for UnsealedTransactionV1 {
    type SignedOutput = Transaction;

    fn into_signed(self, sig: TransactionSealSignature) -> Self::SignedOutput {
        self.set_seal_signature(sig)
    }
}

/// Signs `transaction` with every key in `secrets`, each over the set of all their public keys, and
/// returns the list in canonical order. A key given twice contributes one signature.
pub(crate) fn sign_authorizations<'a, I>(
    transaction: &UnsignedTransactionV1,
    seal_signer: &RistrettoPublicKeyBytes,
    secrets: I,
) -> Vec<TransactionSignature>
where
    I: IntoIterator<Item = &'a RistrettoSecretKey>,
{
    use ootle_byte_type::ToByteType;
    use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};

    let secrets: Vec<_> = secrets.into_iter().collect();
    let authorizers: AuthorizerSet = secrets
        .iter()
        .map(|secret| RistrettoPublicKey::from_secret_key(secret).to_byte_type())
        .collect();
    let message =
        TransactionSignature::create_message(AuthorizationContext::new(seal_signer, &authorizers), transaction);
    let mut signatures = secrets
        .into_iter()
        .map(|secret| TransactionSignature::sign_message(secret, &message))
        .collect();
    canonicalize_authorization_list(&mut signatures);
    signatures
}
