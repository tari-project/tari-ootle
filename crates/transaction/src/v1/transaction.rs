//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt::Display, iter};

use indexmap::IndexSet;
use log::*;
use serde::{Deserialize, Serialize};
use tari_engine_types::{
    hashing::hash_template_code,
    indexed_value::IndexedValueError,
    published_template::PublishedTemplateAddress,
    substate::SubstateId,
};
use tari_ootle_common_types::{Epoch, SubstateRequirement, SubstateRequirementRef};
use tari_template_lib::{
    constants::XTR,
    models::{ComponentAddress, StealthTransferStatement},
};

use crate::{
    args::InstructionArg,
    v1::signature::TransactionSignature,
    weight::TransactionWeight,
    Instruction,
    TransactionSealSignature,
    UnsealedTransactionV1,
};

const LOG_TARGET: &str = "tari::ootle::transaction::transaction";

static XTR_REQUIREMENT: SubstateRequirement = SubstateRequirement::new(SubstateId::Resource(XTR), None);

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionV1 {
    body: UnsealedTransactionV1,
    seal_signature: TransactionSealSignature,
}

impl TransactionV1 {
    pub fn new(transaction: UnsealedTransactionV1, seal_signature: TransactionSealSignature) -> Self {
        Self {
            body: transaction,
            seal_signature,
        }
    }

    pub const fn schema_version(&self) -> u16 {
        self.body.schema_version()
    }

    pub fn is_dry_run(&self) -> bool {
        self.body.is_dry_run()
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
            debug!(target: LOG_TARGET, "Transaction seal signature is invalid");
            return false;
        }

        self.body.verify_all_signatures(self.seal_signature.public_key())
    }

    pub(crate) fn inputs(&self) -> &IndexSet<SubstateRequirement> {
        self.body.inputs()
    }

    pub fn unsealed_transaction(&self) -> &UnsealedTransactionV1 {
        &self.body
    }

    pub fn calculate_transaction_weight(&self) -> TransactionWeight {
        const SIGNER_FACTOR: u64 = 5;
        const INPUT_FACTOR: u64 = 15;
        let num_inputs = self.inputs().len() as u64;
        let num_signers = self.signatures().len() as u64;
        let instruction_weight = self
            .instructions()
            .iter()
            .chain(self.fee_instructions())
            .map(calc_instruction_weight)
            .sum::<TransactionWeight>();
        instruction_weight + (num_inputs * INPUT_FACTOR) + (num_signers * SIGNER_FACTOR)
    }

    pub fn into_unsealed_transaction(self) -> UnsealedTransactionV1 {
        self.body
    }

    pub fn into_parts(self) -> (UnsealedTransactionV1, TransactionSealSignature) {
        (self.body, self.seal_signature)
    }

    pub fn all_inputs_iter(&self) -> impl Iterator<Item = SubstateRequirementRef<'_>> + '_ {
        self.inputs()
            .iter()
            .filter(|id| id.substate_id().as_resource_address() != Some(XTR))
            // Ensure XTR requirement is always included since every transaction needs to pay fees in XTR
            .chain(iter::once(&XTR_REQUIREMENT))
            .map(Into::into)
    }

    pub fn all_published_templates_iter(&self) -> impl Iterator<Item = (PublishedTemplateAddress, &[u8])> + '_ {
        let sealed_pk = self.seal_signature.public_key();
        self.instructions()
            .iter()
            .chain(self.fee_instructions())
            .filter_map(|instruction| {
                if let Instruction::PublishTemplate { binary } = instruction {
                    let binary_hash = hash_template_code(binary);
                    Some((
                        PublishedTemplateAddress::from_author_and_binary_hash(sealed_pk, &binary_hash),
                        binary.as_slice(),
                    ))
                } else {
                    None
                }
            })
    }

    pub fn min_epoch(&self) -> Option<Epoch> {
        self.body.min_epoch()
    }

    pub fn max_epoch(&self) -> Option<Epoch> {
        self.body.max_epoch()
    }

    pub fn as_referenced_components(&self) -> impl Iterator<Item = &ComponentAddress> + '_ {
        self.body.as_referenced_components()
    }

    /// Returns all substates addresses referenced by this transaction
    pub fn to_referenced_substates(&self) -> Result<HashSet<SubstateId>, IndexedValueError> {
        self.body.to_referenced_substates()
    }

    pub fn has_inputs_without_version(&self) -> bool {
        self.inputs().iter().any(|i| i.version().is_none())
    }
}

impl Display for TransactionV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TransactionV1[Inputs: {}, Fee Instructions: {}, Instructions: {}, Signatures: {}]",
            self.body.inputs().len(),
            self.body.fee_instructions().len(),
            self.body.instructions().len(),
            self.signatures().len(),
        )
    }
}

fn calc_instruction_weight(instruction: &Instruction) -> u64 {
    const BINARY_WEIGHT_DIVISOR: u64 = 3;
    // TODO: formalize costing numbers
    const CLAIM_FIXED_COST: u64 = 1100;
    match instruction {
        Instruction::CreateAccount {
            access_rules,
            bucket_workspace_id: workspace_id,
            ..
        } => {
            access_rules.as_ref().map(|a| a.num_access_rules() as u64).unwrap_or(0) +
                workspace_id.as_ref().map(|_| 1).unwrap_or(0)
        },
        Instruction::CallFunction { args, .. } => calc_args_weight(args),
        Instruction::CallMethod { args, .. } => calc_args_weight(args),
        Instruction::PutLastInstructionOutputOnWorkspace { .. } => 0, // Call already costs
        Instruction::EmitLog { message, .. } => message.len() as u64 / BINARY_WEIGHT_DIVISOR,
        Instruction::ClaimBurn { .. } => CLAIM_FIXED_COST,
        Instruction::ClaimValidatorFees { .. } => 1,
        Instruction::DropAllProofsInWorkspace => 1,
        Instruction::AssertBucketContains { .. } => 1,
        Instruction::TakeFromBucket { .. } => 1,
        Instruction::PublishTemplate { binary } => binary.len() as u64 / BINARY_WEIGHT_DIVISOR,
        Instruction::AllocateAddress { .. } => 1,
        Instruction::StealthTransfer { statement, .. } => calc_stealth_statement_weight(statement),
        Instruction::PayFee { statement, .. } => calc_stealth_statement_weight(statement),
        Instruction::UpdateComponentTemplate { migrate, .. } => {
            1 + migrate.as_ref().map(|m| calc_args_weight(&m.args)).unwrap_or(0)
        },
    }
}

fn calc_stealth_statement_weight(statement: &StealthTransferStatement) -> u64 {
    // TODO: weight inputs and outputs accordingly - currently outputs cost 2x inputs
    100 + statement.inputs_statement.inputs.len() as u64 + (statement.outputs_statement.outputs.len() as u64 * 2)
}

fn calc_args_weight(args: &[InstructionArg]) -> u64 {
    const FROM_WORKSPACE_WEIGHT: u64 = 1; // Default weight for args that are not literal bytes
    args.iter()
        .map(|a| {
            a.as_literal_bytes()
                .map_or(FROM_WORKSPACE_WEIGHT, |b| b.len().min(1) as u64)
        })
        .sum()
}
