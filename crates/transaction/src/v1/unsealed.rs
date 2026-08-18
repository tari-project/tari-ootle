//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use indexmap::IndexSet;
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::{indexed_value::IndexedValueError, substate::SubstateId};
use tari_ootle_common_types::{Epoch, SubstateRequirement};
use tari_template_lib_types::{ComponentAddress, crypto::RistrettoPublicKeyBytes};

use crate::{
    BlobHashes,
    Blobs,
    Instruction,
    IntoSigned,
    Transaction,
    TransactionSealSignature,
    signable::Signable,
    v1::{signature::TransactionSignature, transaction::TransactionV1, unsigned::UnsignedTransactionV1},
};

const LOG_TARGET: &str = "tari::ootle::transaction::transaction";

#[derive(Debug, Clone, borsh::BorshSerialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UnsealedTransactionV1 {
    #[n(0)]
    transaction: UnsignedTransactionV1,
    #[n(1)]
    signatures: Vec<TransactionSignature>,
}

impl UnsealedTransactionV1 {
    pub fn new(unsigned_transaction: UnsignedTransactionV1, signatures: Vec<TransactionSignature>) -> Self {
        Self {
            transaction: unsigned_transaction,
            signatures,
        }
    }

    pub const fn schema_version(&self) -> u16 {
        1
    }

    pub fn seal(self, secret: &RistrettoSecretKey) -> Transaction {
        let sig = TransactionSealSignature::sign_v1(secret, &self);
        self.set_seal_signature(sig)
    }

    fn set_seal_signature(self, signature: TransactionSealSignature) -> Transaction {
        TransactionV1::new(self, signature).into()
    }

    pub fn seal_with_signature(self, signature: TransactionSealSignature) -> Transaction {
        self.set_seal_signature(signature)
    }

    pub fn add_signer(mut self, seal_signer: &RistrettoPublicKeyBytes, secret: &RistrettoSecretKey) -> Self {
        let sig = TransactionSignature::sign_v1(secret, seal_signer, &self.transaction);
        self.signatures.push(sig);
        self
    }

    pub(crate) fn add_signature(mut self, signature: TransactionSignature) -> Self {
        self.signatures.push(signature);
        self
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

    pub fn verify_all_signatures(&self, seal_signer: &RistrettoPublicKeyBytes) -> bool {
        self.verify_all_signatures_with_blob_hashes(seal_signer, &self.transaction.blobs.hashes())
    }

    /// Verifies every authorization signature against a transaction whose blob commitments have
    /// already been derived.
    ///
    /// Deriving them hashes every blob payload, so a caller that also verifies the seal should
    /// derive [`Blobs::hashes`] once and use this for both.
    pub fn verify_all_signatures_with_blob_hashes(
        &self,
        seal_signer: &RistrettoPublicKeyBytes,
        blob_hashes: &BlobHashes,
    ) -> bool {
        if self.signatures().is_empty() {
            return true;
        }

        // Derived once and reused: every signature over this transaction signs the same message, and
        // deriving it hashes the whole body including a commitment over every blob's bytes.
        let message =
            TransactionSignature::create_message_v1_with_blob_hashes(seal_signer, &self.transaction, blob_hashes);
        self.signatures().iter().enumerate().all(|(i, sig)| {
            if sig.verify_message(message) {
                true
            } else {
                log::debug!(target: LOG_TARGET, "Failed to verify signature at index {}", i);
                false
            }
        })
    }

    pub fn inputs(&self) -> &IndexSet<SubstateRequirement> {
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
    type MessageOutput = [u8; 64];
    type Signature = TransactionSealSignature;

    fn to_signing_message(&self, _context: ()) -> Self::MessageOutput {
        TransactionSealSignature::create_message_v1(self)
    }
}

impl Signable<&RistrettoPublicKeyBytes> for UnsealedTransactionV1 {
    type MessageOutput = [u8; 64];
    type Signature = TransactionSignature;

    fn to_signing_message(&self, context: &RistrettoPublicKeyBytes) -> Self::MessageOutput {
        TransactionSignature::create_message_v1(context, &self.transaction)
    }
}

impl IntoSigned for UnsealedTransactionV1 {
    type SignedOutput = Transaction;

    fn into_signed(self, sig: TransactionSealSignature) -> Self::SignedOutput {
        self.set_seal_signature(sig)
    }
}

impl IntoSigned<&RistrettoPublicKeyBytes> for UnsealedTransactionV1 {
    type SignedOutput = Self;

    fn into_signed(self, sig: TransactionSignature) -> Self::SignedOutput {
        self.add_signature(sig)
    }
}
