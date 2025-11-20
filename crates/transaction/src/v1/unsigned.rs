//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use tari_engine_types::{
    indexed_value::{IndexedValue, IndexedValueError},
    substate::SubstateId,
};
use tari_ootle_common_types::{Epoch, Signable, SubstateRequirement};
use tari_template_lib::{
    constants::XTR,
    models::{ComponentAddress, UtxoAddress},
    prelude::RistrettoPublicKeyBytes,
};

use crate::{builder::TransactionBuilder, ComponentCall, Instruction, ResourceAddressRef, TransactionSignature};

#[derive(Debug, Clone, Serialize, Deserialize, Default, borsh::BorshSerialize)]
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
}

impl UnsignedTransactionV1 {
    pub fn builder() -> TransactionBuilder {
        TransactionBuilder::new()
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
            is_seal_signer_authorized: false,
            dry_run,
        }
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
            .filter_map(|instruction| {
                if let Instruction::CallMethod {
                    call: ComponentCall::Address(component_address),
                    ..
                } = instruction
                {
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
                    call: ComponentCall::Address(component_address),
                    args,
                    ..
                } => {
                    substates.insert(SubstateId::Component(*component_address));
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
                Instruction::PayFee { statement, .. } => {
                    substates.insert(SubstateId::Resource(XTR));
                    substates.extend(
                        statement
                            .inputs_statement
                            .inputs
                            .iter()
                            .map(|i| UtxoAddress::new(XTR, i.commitment.into()))
                            .map(SubstateId::Utxo),
                    );
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

    fn to_signing_message(&self, seal_signer: &RistrettoPublicKeyBytes) -> Self::MessageOutput {
        TransactionSignature::create_message_v1(1, seal_signer, self)
    }
}
