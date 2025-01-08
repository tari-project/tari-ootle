//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use tari_dan_common_types::{Epoch, SubstateRequirement};
use tari_engine_types::{
    indexed_value::{IndexedValue, IndexedValueError},
    instruction::Instruction,
    substate::SubstateId,
};
use tari_template_lib::models::ComponentAddress;

use crate::UnsignedTransactionV1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum UnsignedTransaction {
    V1(UnsignedTransactionV1),
}

impl UnsignedTransaction {
    pub fn set_network<N: Into<u8>>(&mut self, network: N) -> &mut Self {
        match self {
            Self::V1(tx) => tx.set_network(network),
        };
        self
    }

    pub fn authorized_sealed_signer(&mut self) -> &mut Self {
        match self {
            Self::V1(tx) => tx.is_seal_signer_authorized = true,
        }
        self
    }

    pub fn fee_instructions(&self) -> &[Instruction] {
        match self {
            Self::V1(tx) => tx.fee_instructions(),
        }
    }

    pub(crate) fn fee_instructions_mut(&mut self) -> &mut Vec<Instruction> {
        match self {
            Self::V1(tx) => &mut tx.fee_instructions,
        }
    }

    pub fn instructions(&self) -> &[Instruction] {
        match self {
            Self::V1(tx) => tx.instructions(),
        }
    }

    pub(crate) fn instructions_mut(&mut self) -> &mut Vec<Instruction> {
        match self {
            Self::V1(tx) => &mut tx.instructions,
        }
    }

    pub fn into_instructions(self) -> Vec<Instruction> {
        match self {
            Self::V1(tx) => tx.instructions,
        }
    }

    pub fn inputs(&self) -> &IndexSet<SubstateRequirement> {
        match self {
            Self::V1(tx) => tx.inputs(),
        }
    }

    pub fn min_epoch(&self) -> Option<Epoch> {
        match self {
            Self::V1(tx) => tx.min_epoch(),
        }
    }

    pub fn max_epoch(&self) -> Option<Epoch> {
        match self {
            Self::V1(tx) => tx.max_epoch(),
        }
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

    pub fn set_min_epoch(&mut self, min_epoch: Option<Epoch>) -> &mut Self {
        match self {
            Self::V1(tx) => tx.min_epoch = min_epoch,
        }
        self
    }

    pub fn set_max_epoch(&mut self, max_epoch: Option<Epoch>) -> &mut Self {
        match self {
            Self::V1(tx) => tx.max_epoch = max_epoch,
        }
        self
    }

    pub(crate) fn inputs_mut(&mut self) -> &mut IndexSet<SubstateRequirement> {
        match self {
            Self::V1(tx) => &mut tx.inputs,
        }
    }
}

impl From<UnsignedTransactionV1> for UnsignedTransaction {
    fn from(tx: UnsignedTransactionV1) -> Self {
        Self::V1(tx)
    }
}

impl Default for UnsignedTransaction {
    fn default() -> Self {
        Self::V1(UnsignedTransactionV1::default())
    }
}
