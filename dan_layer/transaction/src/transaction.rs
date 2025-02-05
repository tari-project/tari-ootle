//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt::Display};

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use tari_dan_common_types::{committee::CommitteeInfo, Epoch, SubstateRequirement, VersionedSubstateId};
use tari_engine_types::{
    indexed_value::IndexedValueError,
    instruction::Instruction,
    published_template::PublishedTemplateAddress,
    substate::SubstateId,
};
use tari_template_lib::{models::ComponentAddress, Hash};

use crate::{
    builder::TransactionBuilder,
    transaction_id::TransactionId,
    v1::UnsealedTransactionV1,
    TransactionSealSignature,
    TransactionSignature,
    TransactionV1,
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

    pub fn new(unsigned_transaction: UnsealedTransactionV1, seal_signature: TransactionSealSignature) -> Self {
        Self::V1(TransactionV1::new(unsigned_transaction, seal_signature))
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

    pub fn unsealed_transaction(&self) -> &UnsealedTransactionV1 {
        match self {
            Self::V1(tx) => tx.unsealed_transaction(),
        }
    }

    pub fn hash(&self) -> Hash {
        match self {
            Self::V1(tx) => tx.hash(),
        }
    }

    pub fn network(&self) -> u8 {
        match self {
            Self::V1(tx) => tx.network(),
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

    pub fn seal_signature(&self) -> &TransactionSealSignature {
        match self {
            Self::V1(tx) => tx.seal_signature(),
        }
    }

    pub fn is_seal_signer_authorized(&self) -> bool {
        match self {
            Self::V1(tx) => tx.is_seal_signer_authorized(),
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
            Self::V1(tx) => tx.into_unsealed_transaction().into_instructions(),
        }
    }

    pub fn all_published_templates_iter(&self) -> impl Iterator<Item = PublishedTemplateAddress> + '_ {
        match self {
            Self::V1(tx) => tx.all_published_templates_iter(),
        }
    }

    pub fn into_parts(
        self,
    ) -> (
        UnsealedTransactionV1,
        TransactionSealSignature,
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

    pub fn has_publish_template(&self) -> bool {
        self.instructions()
            .iter()
            .chain(self.fee_instructions())
            .any(|i| matches!(i, Instruction::PublishTemplate { .. }))
    }

    pub fn is_global(&self) -> bool {
        self.has_publish_template()
    }

    pub fn publish_templates_iter(&self) -> impl Iterator<Item = &[u8]> + '_ {
        self.instructions().iter().filter_map(|i| match i {
            Instruction::PublishTemplate { binary } => Some(binary.as_slice()),
            _ => None,
        })
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

    pub const fn schema_version(&self) -> u64 {
        match self {
            Self::V1(tx) => tx.schema_version(),
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

impl From<TransactionV1> for Transaction {
    fn from(tx: TransactionV1) -> Self {
        Self::V1(tx)
    }
}

impl Display for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transaction::V1(tx) => write!(f, "{tx}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;
    use tari_common_types::types::{PrivateKey, PublicKey};
    use tari_crypto::{
        keys::{PublicKey as _, SecretKey},
        tari_utilities::ByteArray,
    };
    use tari_engine_types::TemplateAddress;
    use tari_template_lib::args;

    use super::*;

    fn create_transaction() -> TransactionBuilder {
        Transaction::builder()
            .for_network(123u8)
            .create_account(Default::default())
            .call_method(ComponentAddress::from_array([1; 32]), "method", args![1, 2, 3])
            .call_function(TemplateAddress::from_array([1; 32]), "function", args![1, 2, 3])
            .publish_template(b"template".to_vec())
            .add_input(SubstateRequirement::versioned(
                SubstateId::Component(ComponentAddress::from_array([1; 32])),
                1,
            ))
    }

    #[test]
    fn it_encodes_and_decodes_without_errors() {
        // This test simply checks that there are no serde tags used that can cause encoding/decoding issues with
        // tari_bor
        let subject = create_transaction().build_and_seal(&Default::default());
        let encoded = tari_bor::encode(&subject).unwrap();
        let _decoded = tari_bor::decode::<Transaction>(&encoded).unwrap();
    }

    #[test]
    fn it_correctly_signs_and_verifies() {
        let secret = PrivateKey::random(&mut OsRng);
        let public_key = PublicKey::from_secret_key(&secret);
        let subject = create_transaction().build_and_seal(&secret);
        assert!(subject.verify_all_signatures());

        let secret2 = PrivateKey::random(&mut OsRng);
        let subject = create_transaction()
            .add_signature(&public_key, &secret2)
            .build_and_seal(&secret);
        assert!(subject.verify_all_signatures());
    }
}
