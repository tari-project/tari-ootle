//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use tari_common_types::types::PublicKey;
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_dan_common_types::{Epoch, SubstateRequirement};
use tari_engine_types::{
    indexed_value::{IndexedValue, IndexedValueError},
    instruction::Instruction,
    substate::SubstateId,
};
use tari_template_lib::models::ComponentAddress;

use crate::{
    v1::{signature::TransactionSignature, transaction::TransactionV1, unsigned::UnsignedTransactionV1},
    Transaction,
    TransactionSealSignature,
};

const LOG_TARGET: &str = "tari::dan::transaction::transaction";

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

    pub fn add_signature(mut self, seal_signer: &PublicKey, secret: &RistrettoSecretKey) -> Self {
        let sig = TransactionSignature::sign_v1(secret, seal_signer, &self.transaction);
        self.signatures.push(sig);
        self
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

    pub fn verify_all_signatures(&self, seal_signer: &PublicKey) -> bool {
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

    pub fn fee_claims(&self) -> impl Iterator<Item = (Epoch, PublicKey)> + '_ {
        self.instructions()
            .iter()
            .chain(self.fee_instructions())
            .filter_map(|instruction| {
                if let Instruction::ClaimValidatorFees {
                    epoch,
                    validator_public_key,
                } = instruction
                {
                    Some((Epoch(*epoch), validator_public_key.clone()))
                } else {
                    None
                }
            })
    }

    pub fn min_epoch(&self) -> Option<Epoch> {
        self.transaction.min_epoch
    }

    pub fn max_epoch(&self) -> Option<Epoch> {
        self.transaction.max_epoch
    }

    pub fn as_referenced_components(&self) -> impl Iterator<Item = &ComponentAddress> + '_ {
        self.instructions()
            .iter()
            .chain(self.fee_instructions())
            .filter_map(|instruction| {
                if let Instruction::CallMethod { component_address, .. } = instruction {
                    Some(component_address)
                } else {
                    None
                }
            })
    }

    /// Returns all substates addresses referenced by this transaction
    pub fn to_referenced_substates(&self) -> Result<HashSet<SubstateId>, IndexedValueError> {
        let all_instructions = self.instructions().iter().chain(self.fee_instructions());

        let mut substates = HashSet::new();
        for instruction in all_instructions {
            match instruction {
                Instruction::CallFunction { args, .. } => {
                    for arg in args.iter().filter_map(|a| a.as_literal_bytes()) {
                        let value = IndexedValue::from_raw(arg)?;
                        substates.extend(value.referenced_substates().filter(|id| !id.is_virtual()));
                    }
                },
                Instruction::CallMethod {
                    component_address,
                    args,
                    ..
                } => {
                    substates.insert(SubstateId::Component(*component_address));
                    for arg in args.iter().filter_map(|a| a.as_literal_bytes()) {
                        let value = IndexedValue::from_raw(arg)?;
                        substates.extend(value.referenced_substates().filter(|id| !id.is_virtual()));
                    }
                },
                Instruction::ClaimBurn { claim } => {
                    substates.insert(SubstateId::UnclaimedConfidentialOutput(claim.output_address));
                },
                _ => {},
            }
        }
        Ok(substates)
    }

    pub fn has_inputs_without_version(&self) -> bool {
        self.inputs().iter().any(|i| i.version().is_none())
    }
}
