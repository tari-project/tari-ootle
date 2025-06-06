//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::{indexed_value::IndexedValueError, instruction::Instruction, substate::SubstateId};
use tari_ootle_common_types::{Epoch, SubstateRequirement};
use tari_template_lib::{models::ComponentAddress, types::crypto::RistrettoPublicKeyBytes};

use crate::{
    v1::{signature::TransactionSignature, transaction::TransactionV1, unsigned::UnsignedTransactionV1},
    Transaction,
    TransactionSealSignature,
};

const LOG_TARGET: &str = "tari::ootle::transaction::transaction";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct UnsealedTransactionV1 {
    transaction: UnsignedTransactionV1,
    signatures: Vec<TransactionSignature>,
}

impl UnsealedTransactionV1 {
    pub fn new(unsigned_transaction: UnsignedTransactionV1, signatures: Vec<TransactionSignature>) -> Self {
        Self {
            transaction: unsigned_transaction,
            signatures,
        }
    }

    pub const fn schema_version(&self) -> u64 {
        1
    }

    pub fn seal(self, secret: &RistrettoSecretKey) -> Transaction {
        let sig = TransactionSealSignature::sign(secret, &self);
        TransactionV1::new(self, sig).into()
    }

    pub fn add_signature(mut self, seal_signer: &RistrettoPublicKeyBytes, secret: &RistrettoSecretKey) -> Self {
        let sig = TransactionSignature::sign_v1(secret, seal_signer, &self.transaction);
        self.signatures.push(sig);
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

    pub fn signatures(&self) -> &[TransactionSignature] {
        &self.signatures
    }

    pub fn verify_all_signatures(&self, seal_signer: &RistrettoPublicKeyBytes) -> bool {
        if self.signatures.is_empty() {
            return true;
        }

        self.signatures().iter().enumerate().all(|(i, sig)| {
            if sig.verify(seal_signer, &self.transaction) {
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
    pub fn into_instructions(self) -> (Vec<Instruction>, Vec<Instruction>) {
        (self.transaction.fee_instructions, self.transaction.instructions)
    }

    pub fn min_epoch(&self) -> Option<Epoch> {
        self.transaction.min_epoch
    }

    pub fn max_epoch(&self) -> Option<Epoch> {
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
