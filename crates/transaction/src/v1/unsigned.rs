//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use tari_engine_types::{
    indexed_value::{IndexedValue, IndexedValueError},
    substate::SubstateId,
};
use tari_ootle_common_types::{Epoch, SubstateRequirement};
use tari_template_lib_types::{ComponentAddress, UtxoAddress, crypto::RistrettoPublicKeyBytes};

use crate::{Blobs, ComponentReference, Instruction, ResourceAddressRef, Signable, TransactionSignature};

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UnsignedTransactionV1 {
    pub network: u8,
    pub fee_instructions: Vec<Instruction>,
    pub instructions: Vec<Instruction>,

    /// Input objects that may be read/write
    pub inputs: IndexSet<SubstateRequirement>,
    pub min_epoch: Option<Epoch>,
    pub max_epoch: Option<Epoch>,
    pub is_seal_signer_authorized: bool,
    pub dry_run: bool,

    /// Prunable side-channel of opaque payloads referenced by instructions via `BlobIndex`.
    /// Only the per-blob commitments (`blobs.hashes()`) participate in the signing domain —
    /// raw blob bytes are excluded so that storage layers can drop them without affecting
    /// signature verifiability or transaction id.
    #[serde(default)]
    pub blobs: Blobs,
}

impl UnsignedTransactionV1 {
    pub(crate) fn new_default<N: Into<u8>>(network: N) -> Self {
        Self {
            network: network.into(),
            fee_instructions: vec![],
            instructions: vec![],
            inputs: IndexSet::new(),
            min_epoch: None,
            max_epoch: None,
            is_seal_signer_authorized: true,
            dry_run: false,
            blobs: Blobs::empty(),
        }
    }

    pub fn new<N: Into<u8>>(
        network: N,
        fee_instructions: Vec<Instruction>,
        instructions: Vec<Instruction>,
        inputs: IndexSet<SubstateRequirement>,
        min_epoch: Option<Epoch>,
        max_epoch: Option<Epoch>,
        dry_run: bool,
    ) -> Self {
        Self {
            network: network.into(),
            fee_instructions,
            instructions,
            inputs,
            min_epoch,
            max_epoch,
            is_seal_signer_authorized: true,
            dry_run,
            blobs: Blobs::empty(),
        }
    }

    pub const fn schema_version(&self) -> u16 {
        1
    }

    pub fn set_network<N: Into<u8>>(&mut self, network: N) -> &mut Self {
        self.network = network.into();
        self
    }

    pub fn set_dry_run(&mut self, dry_run: bool) -> &mut Self {
        self.dry_run = dry_run;
        self
    }

    pub fn fee_instructions(&self) -> &[Instruction] {
        &self.fee_instructions
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn inputs(&self) -> &IndexSet<SubstateRequirement> {
        &self.inputs
    }

    pub fn add_input(&mut self, input: SubstateRequirement) -> &mut Self {
        self.inputs.insert(input);
        self
    }

    pub fn blobs(&self) -> &Blobs {
        &self.blobs
    }

    pub fn blobs_mut(&mut self) -> &mut Blobs {
        &mut self.blobs
    }

    /// Validate blob index references and blob coverage. Mirrors
    /// `TransactionV1::validate_blob_references` but operates on the unsigned form so the
    /// wallet daemon can reject malformed input before signing.
    pub fn validate_blob_references(&self) -> Result<(), crate::v1::BlobValidationError> {
        let blob_count = self.blobs.len();
        let mut referenced = vec![false; blob_count];
        for inst in self.instructions().iter().chain(self.fee_instructions()) {
            for idx in inst.referenced_blob_ids() {
                let i = idx as usize;
                if i >= blob_count {
                    return Err(crate::v1::BlobValidationError::IndexOutOfBounds {
                        index: idx,
                        count: blob_count,
                    });
                }
                referenced[i] = true;
            }
        }
        if let Some(unused) = referenced.iter().position(|&r| !r) {
            return Err(crate::v1::BlobValidationError::UnreferencedBlob {
                index: unused as crate::BlobIndex,
            });
        }
        Ok(())
    }

    /// Returns (fee instructions, instructions)
    pub fn into_instructions(self) -> (Vec<Instruction>, Vec<Instruction>) {
        (self.fee_instructions, self.instructions)
    }

    pub fn min_epoch(&self) -> Option<Epoch> {
        self.min_epoch
    }

    pub fn max_epoch(&self) -> Option<Epoch> {
        self.max_epoch
    }

    pub fn as_referenced_components(&self) -> impl Iterator<Item = &ComponentAddress> + '_ {
        self.instructions()
            .iter()
            .chain(self.fee_instructions())
            .filter_map(|instruction| match instruction {
                Instruction::CallMethod {
                    call: ComponentReference::Address(address),
                    ..
                } => Some(address),
                Instruction::UpdateComponentTemplate {
                    component: ComponentReference::Address(address),
                    ..
                } => Some(address),
                _ => None,
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
                Instruction::CallMethod { call, args, .. } => {
                    if let Some(component_address) = call.address() {
                        substates.insert(SubstateId::Component(*component_address));
                    }
                    for arg in args.iter().filter_map(|a| a.as_literal_bytes()) {
                        let value = IndexedValue::from_raw(arg)?;
                        substates.extend(value.referenced_substates().filter(|id| !id.is_virtual()));
                    }
                },
                Instruction::ClaimValidatorFees { address, .. } => {
                    substates.insert(SubstateId::ValidatorFeePool(*address));
                },
                Instruction::StealthTransfer {
                    resource_address_ref: ResourceAddressRef::Address(addr),
                    statement,
                    ..
                } => {
                    substates.insert(SubstateId::Resource(*addr));
                    substates.extend(
                        statement
                            .inputs_statement
                            .inputs
                            .iter()
                            .map(|i| UtxoAddress::new(*addr, i.commitment.into()))
                            .map(SubstateId::Utxo),
                    );
                },
                Instruction::UpdateComponentTemplate { component, migrate, .. } => {
                    if let Some(component_address) = component.address() {
                        substates.insert(SubstateId::Component(*component_address));
                    }
                    for arg in migrate
                        .as_ref()
                        .iter()
                        .flat_map(|m| &m.args)
                        .filter_map(|a| a.as_literal_bytes())
                    {
                        let value = IndexedValue::from_raw(arg)?;
                        substates.extend(value.referenced_substates().filter(|id| !id.is_virtual()));
                    }
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

impl Signable<&RistrettoPublicKeyBytes> for UnsignedTransactionV1 {
    type MessageOutput = [u8; 64];
    type Signature = TransactionSignature;

    fn to_signing_message(&self, seal_signer: &RistrettoPublicKeyBytes) -> Self::MessageOutput {
        TransactionSignature::create_message_v1(seal_signer, self)
    }
}
