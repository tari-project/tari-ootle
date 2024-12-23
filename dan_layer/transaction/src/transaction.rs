//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt::Display};

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use tari_common_types::types::PublicKey;
use tari_crypto::ristretto::RistrettoSecretKey;
use tari_dan_common_types::{committee::CommitteeInfo, Epoch, SubstateRequirement, VersionedSubstateId};
use tari_engine_types::{indexed_value::IndexedValueError, instruction::Instruction, substate::SubstateId};
use tari_template_lib::{models::ComponentAddress, Hash};

use crate::{
    builder::TransactionBuilder,
    transaction_id::TransactionId,
    v1::TransactionV1,
    TransactionSignature,
    UnsignedTransaction,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../bindings/src/types/")
)]
pub enum Transaction {
    V1(TransactionV1),
}

impl Transaction {
    pub fn builder() -> TransactionBuilder {
        TransactionBuilder::new()
    }

    pub fn new(unsigned_transaction: UnsignedTransaction, signatures: Vec<TransactionSignature>) -> Self {
        Self::V1(TransactionV1::new(unsigned_transaction, signatures))
    }

    pub fn sign(self, secret: &RistrettoSecretKey) -> Self {
        match self {
            Self::V1(tx) => Self::V1(tx.sign(secret)),
        }
    }

    pub fn with_filled_inputs(self, filled_inputs: IndexSet<VersionedSubstateId>) -> Self {
        match self {
            Self::V1(tx) => Self::V1(tx.with_filled_inputs(filled_inputs)),
        }
    }

    pub fn id(&self) -> &TransactionId {
        match self {
            Self::V1(tx) => tx.id(),
        }
    }

    pub fn check_id(&self) -> bool {
        match self {
            Self::V1(tx) => tx.check_id(),
        }
    }

    pub fn unsigned_transaction(&self) -> &UnsignedTransaction {
        match self {
            Self::V1(tx) => tx.unsigned_transaction(),
        }
    }

    pub fn hash(&self) -> Hash {
        match self {
            Self::V1(tx) => tx.hash(),
        }
    }

    pub fn fee_instructions(&self) -> &[Instruction] {
        match self {
            Self::V1(tx) => tx.fee_instructions(),
        }
    }

    pub fn instructions(&self) -> &[Instruction] {
        match self {
            Self::V1(tx) => tx.instructions(),
        }
    }

    pub fn signatures(&self) -> &[TransactionSignature] {
        match self {
            Self::V1(tx) => tx.signatures(),
        }
    }

    pub fn verify_all_signatures(&self) -> bool {
        match self {
            Self::V1(tx) => tx.verify_all_signatures(),
        }
    }

    pub fn inputs(&self) -> &IndexSet<SubstateRequirement> {
        match self {
            Self::V1(tx) => tx.inputs(),
        }
    }

    /// Returns (fee instructions, instructions)
    pub fn into_instructions(self) -> (Vec<Instruction>, Vec<Instruction>) {
        match self {
            Self::V1(tx) => tx.into_instructions(),
        }
    }

    pub fn into_parts(
        self,
    ) -> (
        UnsignedTransaction,
        Vec<TransactionSignature>,
        IndexSet<VersionedSubstateId>,
    ) {
        match self {
            Self::V1(tx) => tx.into_parts(),
        }
    }

    pub fn all_inputs_iter(&self) -> impl Iterator<Item = SubstateRequirement> + '_ {
        match self {
            Self::V1(tx) => tx.all_inputs_iter(),
        }
    }

    pub fn all_inputs_substate_ids_iter(&self) -> impl Iterator<Item = &SubstateId> + '_ {
        self.inputs()
            .iter()
            // Filled inputs override other inputs as they are likely filled with versions
            .filter(|i| self.filled_inputs().iter().all(|fi| fi.substate_id() != i.substate_id()))
            .map(|i| i.substate_id())
            .chain(self.filled_inputs().iter().map(|fi| fi.substate_id()))
    }

    /// Returns true if the provided committee is involved in at least one input of this transaction.
    pub fn is_involved_inputs(&self, committee_info: &CommitteeInfo) -> bool {
        self.all_inputs_iter()
            .any(|id| committee_info.includes_substate_id(id.substate_id()))
    }

    pub fn num_unique_inputs(&self) -> usize {
        self.all_inputs_substate_ids_iter().count()
    }

    pub fn filled_inputs(&self) -> &IndexSet<VersionedSubstateId> {
        match self {
            Self::V1(tx) => tx.filled_inputs(),
        }
    }

    pub fn filled_inputs_mut(&mut self) -> &mut IndexSet<VersionedSubstateId> {
        match self {
            Self::V1(tx) => tx.filled_inputs_mut(),
        }
    }

    pub fn fee_claims(&self) -> impl Iterator<Item = (Epoch, PublicKey)> + '_ {
        match self {
            Self::V1(tx) => tx.fee_claims(),
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

    pub fn schema_version(&self) -> u64 {
        match self {
            Self::V1(_) => 1,
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
        match self {
            Self::V1(tx) => tx.has_inputs_without_version(),
        }
    }
}

impl Display for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transaction::V1(tx) => write!(f, "{tx}"),
        }
    }
}
