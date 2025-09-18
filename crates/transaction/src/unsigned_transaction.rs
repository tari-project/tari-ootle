//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashSet;

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use tari_engine_types::{indexed_value::IndexedValueError, instruction::Instruction, substate::SubstateId};
use tari_ootle_common_types::{Epoch, SubstateRequirement};
use tari_template_lib::models::ComponentAddress;

use crate::{TransactionSignature, UnsealedTransactionV1, UnsignedTransactionV1};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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

    pub fn set_dry_run(&mut self, dry_run: bool) -> &mut Self {
        match self {
            Self::V1(tx) => tx.set_dry_run(dry_run),
        };
        self
    }

    pub fn authorized_sealed_signer(mut self) -> Self {
        match self {
            Self::V1(ref mut tx) => tx.is_seal_signer_authorized = true,
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
        match self {
            Self::V1(tx) => tx.as_referenced_components(),
        }
    }

    /// Returns all substates addresses referenced by this transaction
    pub fn to_referenced_substates(&self) -> Result<HashSet<SubstateId>, IndexedValueError> {
        match self {
            Self::V1(tx) => tx.to_referenced_substates(),
        }
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

    pub fn with_inputs<I: IntoIterator<Item = SubstateRequirement>>(mut self, inputs: I) -> Self {
        self.inputs_mut().extend(inputs);
        self
    }

    pub fn build(self, signatures: Vec<TransactionSignature>) -> UnsealedTransactionV1 {
        // Obviously this will not work if we have more than one version - dealing with that is left for another time
        match self {
            UnsignedTransaction::V1(tx) => UnsealedTransactionV1::new(tx, signatures),
        }
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
