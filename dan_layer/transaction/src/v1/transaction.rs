//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt::Display};

use indexmap::IndexSet;
use log::*;
use serde::{Deserialize, Serialize};
use tari_dan_common_types::{
    Epoch,
    NumPreshards,
    ShardGroup,
    SubstateAddress,
    SubstateRequirement,
    VersionedSubstateId,
};
use tari_engine_types::{
    hashing::{hasher32, EngineHashDomainLabel},
    indexed_value::{IndexedValue, IndexedValueError},
    instruction::Instruction,
    published_template::PublishedTemplateAddress,
    substate::SubstateId,
};
use tari_template_lib::{models::ComponentAddress, Hash};

use crate::{v1::signature::TransactionSignature, TransactionId, TransactionSealSignature, UnsealedTransactionV1};

const LOG_TARGET: &str = "tari::dan::transaction::transaction";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub struct TransactionV1 {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    id: TransactionId,
    body: UnsealedTransactionV1,
    seal_signature: TransactionSealSignature,
    /// Inputs filled by some authority. These are not part of the transaction hash nor the signature
    filled_inputs: IndexSet<VersionedSubstateId>,
}

impl TransactionV1 {
    pub fn new(transaction: UnsealedTransactionV1, seal_signature: TransactionSealSignature) -> Self {
        let mut tx = Self {
            id: TransactionId::default(),
            body: transaction,
            seal_signature,
            filled_inputs: IndexSet::new(),
        };
        tx.id = tx.calculate_hash();
        tx
    }

    pub const fn schema_version(&self) -> u64 {
        self.body.schema_version()
    }

    pub fn with_filled_inputs(self, filled_inputs: IndexSet<VersionedSubstateId>) -> Self {
        Self { filled_inputs, ..self }
    }

    fn calculate_hash(&self) -> TransactionId {
        hasher32(EngineHashDomainLabel::Transaction)
            .chain(&self.seal_signature)
            .chain(&self.body)
            .result()
            .into_array()
            .into()
    }

    pub fn id(&self) -> &TransactionId {
        &self.id
    }

    pub fn check_id(&self) -> bool {
        let id = self.calculate_hash();
        id == self.id
    }

    pub fn hash(&self) -> Hash {
        self.id.into_array().into()
    }

    pub fn network(&self) -> u8 {
        self.body.unsigned_transaction().network
    }

    pub fn fee_instructions(&self) -> &[Instruction] {
        self.body.fee_instructions()
    }

    pub fn instructions(&self) -> &[Instruction] {
        self.body.instructions()
    }

    pub fn signatures(&self) -> &[TransactionSignature] {
        self.body.signatures()
    }

    pub fn seal_signature(&self) -> &TransactionSealSignature {
        &self.seal_signature
    }

    pub fn is_seal_signer_authorized(&self) -> bool {
        self.body.unsigned_transaction().is_seal_signer_authorized
    }

    pub fn verify_all_signatures(&self) -> bool {
        if !self.seal_signature.verify(&self.body) {
            debug!(target: LOG_TARGET, "Transaction seal signature is valid");
            return false;
        }

        self.body.verify_all_signatures(self.seal_signature.public_key())
    }

    pub fn inputs(&self) -> &IndexSet<SubstateRequirement> {
        self.body.inputs()
    }

    pub fn unsealed_transaction(&self) -> &UnsealedTransactionV1 {
        &self.body
    }

    pub fn into_unsealed_transaction(self) -> UnsealedTransactionV1 {
        self.body
    }

    pub fn into_parts(
        self,
    ) -> (
        UnsealedTransactionV1,
        TransactionSealSignature,
        IndexSet<VersionedSubstateId>,
    ) {
        (self.body, self.seal_signature, self.filled_inputs)
    }

    pub fn all_inputs_iter(&self) -> impl Iterator<Item = SubstateRequirement> + '_ {
        self.inputs()
            .iter()
            // Filled inputs override other inputs as they are likely filled with versions
            .filter(|i| self.filled_inputs().iter().all(|fi| fi.substate_id() != i.substate_id()))
            .cloned()
            .chain(self.filled_inputs().iter().cloned().map(Into::into))
    }

    pub fn all_inputs_substate_ids_iter(&self) -> impl Iterator<Item = &SubstateId> + '_ {
        self.inputs()
            .iter()
            // Filled inputs override other inputs as they are likely filled with versions
            .filter(|i| self.filled_inputs().iter().all(|fi| fi.substate_id() != i.substate_id()))
            .map(|i| i.substate_id())
            .chain(self.filled_inputs().iter().map(|fi| fi.substate_id()))
    }

    pub fn to_all_involved_shards(&self, num_shards: NumPreshards, num_committees: u32) -> HashSet<ShardGroup> {
        self.all_inputs_substate_ids_iter()
            .map(|id| {
                // version doesnt affect shard
                let addr = SubstateAddress::from_substate_id(id, 0);
                addr.to_shard_group(num_shards, num_committees)
            })
            .collect()
    }

    pub fn all_published_templates_iter(&self) -> impl Iterator<Item = PublishedTemplateAddress> + '_ {
        let sealed_pk = self.seal_signature.public_key();
        self.instructions()
            .iter()
            .chain(self.fee_instructions())
            .filter_map(|instruction| {
                if let Instruction::PublishTemplate { binary } = instruction {
                    Some(PublishedTemplateAddress::from_author_and_code(sealed_pk, binary))
                } else {
                    None
                }
            })
    }

    pub fn filled_inputs(&self) -> &IndexSet<VersionedSubstateId> {
        &self.filled_inputs
    }

    pub fn filled_inputs_mut(&mut self) -> &mut IndexSet<VersionedSubstateId> {
        &mut self.filled_inputs
    }

    pub fn min_epoch(&self) -> Option<Epoch> {
        self.body.min_epoch()
    }

    pub fn max_epoch(&self) -> Option<Epoch> {
        self.body.max_epoch()
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
                Instruction::ClaimValidatorFees { address, .. } => {
                    substates.insert(SubstateId::ValidatorFeePool(*address));
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

impl Display for TransactionV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TransactionV1[{}, Inputs: {}, Fee Instructions: {}, Instructions: {}, Signatures: {}, Filled Inputs: {}]",
            self.id,
            self.body.inputs().len(),
            self.body.fee_instructions().len(),
            self.body.instructions().len(),
            self.signatures().len(),
            self.filled_inputs.len(),
        )
    }
}
