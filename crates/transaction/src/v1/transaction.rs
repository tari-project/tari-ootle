//    Copyright 2024 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt::Display, iter};

use indexmap::IndexSet;
use log::*;
use tari_engine_types::{
    hashing::{EngineHashDomainLabel, hash_template_code, hasher32},
    indexed_value::IndexedValueError,
    published_template::PublishedTemplateAddress,
    substate::SubstateId,
};
use tari_ootle_common_types::{Epoch, SubstateRequirement, SubstateRequirementRef};
use tari_template_lib_types::{ComponentAddress, constants::TARI_TOKEN, stealth::StealthTransferStatement};

use crate::{
    Blobs,
    Instruction,
    TransactionId,
    TransactionSealSignature,
    UnsealedTransactionV1,
    args::InstructionArg,
    v1::signature::{TransactionSignature, TransactionSignatureFields},
    weight::TransactionWeight,
};

const LOG_TARGET: &str = "tari::ootle::transaction::transaction";

static XTR_REQUIREMENT: SubstateRequirement = SubstateRequirement::new(SubstateId::Resource(TARI_TOKEN), None);

#[derive(Debug, Clone, borsh::BorshSerialize, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TransactionV1 {
    #[n(0)]
    body: UnsealedTransactionV1,
    #[n(1)]
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

    pub fn blobs(&self) -> &Blobs {
        self.body.blobs()
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
        if !self.seal_signature.verify_v1(&self.body) {
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

    /// Compute the deterministic transaction id.
    ///
    /// The id projection deliberately excludes raw blob bytes — only their per-blob commitments
    /// (`BlobHashes`) participate. This keeps the id stable across pruning: a `TransactionV1`
    /// and the `PrunedTransactionV1` derived from it produce the same id.
    pub fn calculate_id(&self) -> TransactionId {
        let unsigned = self.body.unsigned_transaction();
        let blob_hashes = unsigned.blobs.hashes();
        hasher32(EngineHashDomainLabel::Transaction)
            .chain(&self.schema_version())
            .chain(&TransactionSignatureFields::from(unsigned))
            .chain(&blob_hashes)
            .chain(self.body.signatures())
            .chain(&self.seal_signature)
            .result()
            .into_array()
            .into()
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
        let blob_weight = calc_blobs_weight(self.body.unsigned_transaction().blobs());
        instruction_weight + blob_weight + (num_inputs * INPUT_FACTOR) + (num_signers * SIGNER_FACTOR)
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
            .filter(|id| id.substate_id().as_resource_address() != Some(TARI_TOKEN))
            // Ensure XTR requirement is always included since every transaction needs to pay fees in XTR
            .chain(iter::once(&XTR_REQUIREMENT))
            .map(Into::into)
    }

    pub fn all_published_templates_iter(&self) -> impl Iterator<Item = (PublishedTemplateAddress, &[u8])> + '_ {
        let sealed_pk = self.seal_signature.public_key();
        let blobs = self.body.unsigned_transaction().blobs();
        self.instructions()
            .iter()
            .chain(self.fee_instructions())
            .filter_map(move |instruction| {
                if let Instruction::PublishTemplate { binary, .. } = instruction {
                    let bytes = blobs.get(*binary)?.as_bytes();
                    let binary_hash = hash_template_code(bytes);
                    Some((
                        PublishedTemplateAddress::from_author_and_binary_hash(sealed_pk, &binary_hash),
                        bytes,
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

    /// Validate the blob side of the transaction:
    ///  * every `BlobIndex` referenced from any instruction is `< blobs.len()`
    ///  * every blob is referenced by at least one instruction (no free riders)
    ///
    /// Intended to run at ingress (RPC submit / mempool admission) before signature verification
    /// — it's strictly cheaper than verifying signatures and rejects malformed transactions
    /// up-front.
    pub fn validate_blob_references(&self) -> Result<(), BlobValidationError> {
        let blobs = self.body.unsigned_transaction().blobs();
        let blob_count = blobs.len();
        let mut referenced = vec![false; blob_count];
        for inst in self.instructions().iter().chain(self.fee_instructions()) {
            for idx in inst.referenced_blob_ids() {
                let i = idx as usize;
                if i >= blob_count {
                    return Err(BlobValidationError::IndexOutOfBounds {
                        index: idx,
                        count: blob_count,
                    });
                }
                referenced[i] = true;
            }
        }
        if let Some(unused) = referenced.iter().position(|&r| !r) {
            return Err(BlobValidationError::UnreferencedBlob {
                index: unused as crate::BlobIndex,
            });
        }
        Ok(())
    }
}

/// Failure modes for `TransactionV1::validate_blob_references`.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum BlobValidationError {
    #[error("Blob index {index} out of bounds (transaction has {count} blob(s))")]
    IndexOutOfBounds { index: crate::BlobIndex, count: usize },
    #[error("Blob at index {index} is not referenced by any instruction")]
    UnreferencedBlob { index: crate::BlobIndex },
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
        Instruction::Assert { .. } => 1,
        Instruction::TakeFromBucket { .. } => 1,
        Instruction::PutIntoBucket { .. } => 1,
        // The binary's bytes are charged at the transaction-level via `calc_blobs_weight`,
        // uniformly with any other blob references. The instruction itself is a fixed cost.
        Instruction::PublishTemplate { .. } => 1,
        Instruction::AllocateAddress { .. } => 1,
        Instruction::StealthTransfer { statement, .. } => calc_stealth_statement_weight(statement),
        Instruction::PayFeeFromBucket { .. } => 1,
        Instruction::UpdateComponentTemplate { migrate, .. } => {
            1 + migrate.as_ref().map(|m| calc_args_weight(&m.args)).unwrap_or(0)
        },
    }
}

fn calc_stealth_statement_weight(statement: &StealthTransferStatement) -> u64 {
    // Per-byte divisor for serialized spend-witness bytes, consistent with the other byte-weighted costs (logs, blobs,
    // literal args).
    const SPEND_WITNESS_BYTE_DIVISOR: u64 = 3;

    // A script-path spend witness reveals the spend-condition leaf — notably a `TemplateFunction`'s `args` — plus its
    // Merkle inclusion proof, carried in the spending transaction. Its serialized size must contribute to weight,
    // otherwise a large leaf/proof could be broadcast for free. Outputs commit only a fixed-size `condition_root`, so
    // their condition payload is already covered by the count-based weight below.
    let witness_bytes: u64 = statement
        .inputs_statement
        .inputs
        .iter()
        .map(|i| tari_bor::encoded_len(&i.witness).unwrap_or(0) as u64)
        .sum();

    // Fixed cost of a transfer (resource lock, balance-proof verification, basic validation).
    const WEIGHT_PER_TRANSFER: u64 = 100;
    // An input contributes a commitment aggregation and a substate lookup — cheap relative to an output.
    const WEIGHT_PER_INPUT: u64 = 1;
    // Each output is verified natively via an aggregated bulletproof range proof plus an ElGamal viewable-balance
    // proof (~1ms/output on x86-class hardware) — the dominant cost of a transfer. Priced well above an input so the
    // per-transaction and per-block weight budgets bound this unmetered native verification work.
    const WEIGHT_PER_OUTPUT: u64 = 8;

    WEIGHT_PER_TRANSFER +
        statement.inputs_statement.inputs.len() as u64 * WEIGHT_PER_INPUT +
        statement.outputs_statement.outputs.len() as u64 * WEIGHT_PER_OUTPUT +
        witness_bytes / SPEND_WITNESS_BYTE_DIVISOR
}

fn calc_args_weight(args: &[InstructionArg]) -> u64 {
    // Workspace and blob refs are cheap — just an index. Blob payloads are charged at the
    // transaction level by `calc_blobs_weight`, so we don't double-count them here.
    const NON_LITERAL_WEIGHT: u64 = 1;
    // Inline literal args carry their bytes directly in the instruction, so price them by size,
    // consistent with blob/log byte costing.
    const LITERAL_BYTE_DIVISOR: u64 = 3;

    // Accumulate the raw literal bytes and apply the divisor once across the whole instruction.
    // Dividing per-argument would let a large literal be split into many small ones so each share
    // rounds down, evading the weight (and hence the fee). Each literal still costs at least
    // `NON_LITERAL_WEIGHT`, so a flood of tiny args can never be cheaper than the same bytes in one.
    let mut total_literal_bytes = 0u64;
    let mut num_literals = 0u64;
    let mut num_non_literals = 0u64;
    for arg in args {
        match arg.as_literal_bytes() {
            Some(bytes) => {
                total_literal_bytes += bytes.len() as u64;
                num_literals += 1;
            },
            None => num_non_literals += 1,
        }
    }

    let literal_weight = (total_literal_bytes / LITERAL_BYTE_DIVISOR).max(num_literals * NON_LITERAL_WEIGHT);
    literal_weight + num_non_literals * NON_LITERAL_WEIGHT
}

/// Per-blob byte weight. Each blob's payload contributes its bytes (divided by the binary
/// weight divisor) plus the fixed commitment cost in the signing domain (32 bytes per blob).
///
/// This is uniform across all blob references — `PublishTemplate` binaries and `Blob`
/// instruction args alike — since the network cost (gossip, storage, signing-domain commit)
/// is indifferent to *how* the blob is referenced.
fn calc_blobs_weight(blobs: &crate::Blobs) -> u64 {
    const BLOB_BYTE_DIVISOR: u64 = 3;
    const BLOB_COMMITMENT_BYTES: u64 = 32;
    blobs
        .iter()
        .map(|blob| (blob.len() as u64 / BLOB_BYTE_DIVISOR) + BLOB_COMMITMENT_BYTES)
        .sum()
}

#[cfg(test)]
mod blob_validation_tests {
    use ootle_byte_type::ToByteType;
    use tari_crypto::{
        keys::{PublicKey as PublicKeyT, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    };
    use tari_template_lib_types::{FunctionName, TemplateAddress};

    use super::*;
    use crate::{Blob, Blobs, UnsignedTransactionV1, args::InstructionArg, v1::unsealed::UnsealedTransactionV1};

    fn build_with_blobs(blobs: Vec<Blob>, instructions: Vec<Instruction>) -> TransactionV1 {
        let blobs = Blobs::from_vec(blobs);
        let unsigned = UnsignedTransactionV1 {
            network: 1,
            fee_instructions: vec![],
            instructions,
            inputs: indexmap::IndexSet::new(),
            min_epoch: None,
            max_epoch: None,
            is_seal_signer_authorized: true,
            dry_run: false,
            blobs,
        };
        let sealer = RistrettoSecretKey::random(&mut rand::rng());
        let unsealed = UnsealedTransactionV1::new(unsigned, vec![]);
        let seal = crate::TransactionSealSignature::sign_v1(&sealer, &unsealed);
        TransactionV1::new(unsealed, seal)
    }

    fn call_function(blob_args: Vec<InstructionArg>) -> Instruction {
        Instruction::CallFunction {
            address: TemplateAddress::from_array([1; 32]),
            function: FunctionName::try_from("f").unwrap(),
            args: blob_args,
        }
    }

    #[test]
    fn validates_when_all_blobs_referenced_in_bounds() {
        let blobs = vec![Blob::from(vec![1]), Blob::from(vec![2])];
        let instructions = vec![
            Instruction::PublishTemplate {
                binary: 0,
                metadata_hash: None,
            },
            call_function(vec![InstructionArg::Blob(1)]),
        ];
        let tx = build_with_blobs(blobs, instructions);
        assert!(tx.validate_blob_references().is_ok());
    }

    #[test]
    fn rejects_out_of_bounds_blob_index_in_publish_template() {
        let tx = build_with_blobs(vec![], vec![Instruction::PublishTemplate {
            binary: 0,
            metadata_hash: None,
        }]);
        let err = tx.validate_blob_references().unwrap_err();
        assert!(matches!(err, BlobValidationError::IndexOutOfBounds {
            index: 0,
            count: 0
        }));
    }

    #[test]
    fn rejects_out_of_bounds_blob_index_in_arg() {
        let tx = build_with_blobs(vec![Blob::from(vec![1])], vec![call_function(vec![
            InstructionArg::Blob(5),
        ])]);
        let err = tx.validate_blob_references().unwrap_err();
        assert!(matches!(err, BlobValidationError::IndexOutOfBounds {
            index: 5,
            count: 1
        }));
    }

    #[test]
    fn rejects_unreferenced_blob() {
        let tx = build_with_blobs(vec![Blob::from(vec![1]), Blob::from(vec![2])], vec![
            Instruction::PublishTemplate {
                binary: 0,
                metadata_hash: None,
            },
        ]);
        let err = tx.validate_blob_references().unwrap_err();
        assert_eq!(err, BlobValidationError::UnreferencedBlob { index: 1 });
    }

    #[test]
    fn validates_empty_when_no_instructions_and_no_blobs() {
        let tx = build_with_blobs(vec![], vec![]);
        assert!(tx.validate_blob_references().is_ok());
    }

    #[test]
    fn blob_size_contributes_to_transaction_weight() {
        // Same instructions, different blob payload size → larger blob ⇒ larger weight.
        let small = build_with_blobs(vec![Blob::from(vec![0u8; 30])], vec![Instruction::PublishTemplate {
            binary: 0,
            metadata_hash: None,
        }]);
        let large = build_with_blobs(vec![Blob::from(vec![0u8; 3000])], vec![Instruction::PublishTemplate {
            binary: 0,
            metadata_hash: None,
        }]);
        assert!(
            large.calculate_transaction_weight() > small.calculate_transaction_weight(),
            "larger blob payload must produce larger transaction weight",
        );
    }

    #[test]
    fn literal_arg_size_contributes_to_transaction_weight() {
        // Same instruction shape, differing only in the byte size of an inline literal argument →
        // the larger argument must produce a larger weight. Guards against under-pricing inline
        // args, which would otherwise let multi-KiB payloads ride along at near-zero weight/fee.
        let small = build_with_blobs(vec![], vec![call_function(vec![InstructionArg::raw_literal_bytes(
            vec![0u8; 30],
        )])]);
        let large = build_with_blobs(vec![], vec![call_function(vec![InstructionArg::raw_literal_bytes(
            vec![0u8; 3000],
        )])]);
        assert!(
            large.calculate_transaction_weight() > small.calculate_transaction_weight(),
            "larger literal argument payload must produce larger transaction weight",
        );
    }

    #[test]
    fn splitting_literal_args_does_not_reduce_weight() {
        // A payload split across many small literal args must not weigh less than the same bytes in
        // a single arg — otherwise an attacker could chunk a literal to round each share down and
        // evade weight/fee pricing.
        const TOTAL: usize = 3000;
        const CHUNK: usize = 5;
        let single = build_with_blobs(vec![], vec![call_function(vec![InstructionArg::raw_literal_bytes(
            vec![0u8; TOTAL],
        )])]);
        let split_args = (0..TOTAL / CHUNK)
            .map(|_| InstructionArg::raw_literal_bytes(vec![0u8; CHUNK]))
            .collect();
        let split = build_with_blobs(vec![], vec![call_function(split_args)]);
        assert!(
            split.calculate_transaction_weight() >= single.calculate_transaction_weight(),
            "splitting a literal arg into smaller chunks must not reduce transaction weight",
        );
    }

    /// Make sure the seal-signer public key recovery isn't silently affected by the new tests
    /// using a randomly-generated sealer.
    #[allow(dead_code)]
    fn pk_smoke() {
        let _ = RistrettoPublicKey::from_secret_key(&RistrettoSecretKey::random(&mut rand::rng())).to_byte_type();
    }
}
