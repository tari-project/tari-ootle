//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashSet, fmt::Display};

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use tari_engine_types::{
    confidential::MinotariBurnClaimProof,
    hashing::{hasher32, EngineHashDomainLabel},
    indexed_value::IndexedValueError,
    published_template::PublishedTemplateAddress,
    substate::SubstateId,
};
use tari_ootle_common_types::{
    committee::CommitteeInfo,
    Epoch,
    SubstateAddress,
    SubstateRequirement,
    SubstateRequirementRef,
    ToSubstateAddress,
    VersionedSubstateId,
};
use tari_template_lib::{
    models::{ClaimedOutputTombstoneAddress, ComponentAddress},
    prelude::TemplateAddress,
};

use crate::{
    builder::TransactionBuilder,
    transaction_id::TransactionId,
    v1::UnsealedTransactionV1,
    weight::TransactionWeight,
    Instruction,
    TransactionSealSignature,
    TransactionSignature,
    TransactionV1,
};

#[derive(Debug, Clone, Serialize, Deserialize, borsh::BorshSerialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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

    pub fn calculate_id(&self) -> TransactionId {
        hasher32(EngineHashDomainLabel::Transaction)
            .chain(&self)
            .result()
            .into_array()
            .into()
    }

    pub fn calculate_transaction_weight(&self) -> TransactionWeight {
        match self {
            Self::V1(tx) => tx.calculate_transaction_weight(),
        }
    }

    pub fn is_dry_run(&self) -> bool {
        match self {
            Transaction::V1(tx) => tx.is_dry_run(),
        }
    }

    pub fn unsealed_transaction(&self) -> &UnsealedTransactionV1 {
        match self {
            Self::V1(tx) => tx.unsealed_transaction(),
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
    pub fn into_instruction_parts(self) -> (Vec<Instruction>, Vec<Instruction>) {
        match self {
            Self::V1(tx) => tx.into_unsealed_transaction().into_instruction_parts(),
        }
    }

    pub fn all_published_templates_iter(&self) -> impl Iterator<Item = (PublishedTemplateAddress, &[u8])> + '_ {
        match self {
            Self::V1(tx) => tx.all_published_templates_iter(),
        }
    }

    pub fn into_parts(self) -> (UnsealedTransactionV1, TransactionSealSignature) {
        match self {
            Self::V1(tx) => tx.into_parts(),
        }
    }

    pub fn all_inputs_iter(&self) -> impl Iterator<Item = SubstateRequirementRef<'_>> + '_ {
        match self {
            Self::V1(tx) => tx.all_inputs_iter(),
        }
    }

    pub fn involved_substate_addresses_iter(&self) -> impl Iterator<Item = SubstateAddress> + '_ {
        self
            .all_inputs_iter()
            // The version does not affect the shard group
            .map(|i| i.or_zero_version().to_substate_address())
            // We define involvement as either being an input or a known output
            .chain(self.known_output_addresses_iter())
    }

    pub fn claim_burn_outputs_iter(&self) -> impl Iterator<Item = ClaimedOutputTombstoneAddress> + '_ {
        self.claim_burn_iter()
            .map(|c| ClaimedOutputTombstoneAddress::from_commitment(c.commitment))
    }

    pub fn claim_burn_iter(&self) -> impl Iterator<Item = &MinotariBurnClaimProof> + '_ {
        self.instructions()
            .iter()
            .chain(self.fee_instructions())
            .filter_map(|i| i.claim_burn())
    }

    pub fn all_inputs_substate_ids_iter(&self) -> impl Iterator<Item = &SubstateId> + '_ {
        self.inputs().iter().map(|i| i.substate_id())
    }

    /// Returns true if the provided committee is involved in at least one input or known output of this transaction.
    /// A committee may be involved even if this function returns false if and only if it is involved in outputs only.
    pub fn is_involved(&self, committee_info: &CommitteeInfo) -> bool {
        if self.is_global() {
            return true;
        }

        self.involved_substate_addresses_iter()
            .any(|addr| committee_info.includes_substate_address(&addr))
    }

    pub fn known_output_addresses_iter(&self) -> impl Iterator<Item = SubstateAddress> + '_ {
        let tx_substate_address = self.calculate_id().to_substate_address();
        std::iter::once(tx_substate_address).chain(
            self.claim_burn_outputs_iter()
                .map(|c| SubstateAddress::from_object_key(c.as_object_key(), 0)),
        )
    }

    pub fn known_outputs_iter(&self) -> impl Iterator<Item = VersionedSubstateId> + '_ {
        let tx_receipt = self.calculate_id().into_receipt_address();
        std::iter::once(VersionedSubstateId::new(tx_receipt, 0)).chain(
            self.claim_burn_outputs_iter()
                .map(SubstateId::from)
                .map(|s| VersionedSubstateId::new(s, 0)),
        )
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

    pub fn num_inputs(&self) -> usize {
        self.all_inputs_substate_ids_iter().count()
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

    pub const fn schema_version(&self) -> u16 {
        match self {
            Self::V1(tx) => tx.schema_version(),
        }
    }

    pub fn as_referenced_components(&self) -> impl Iterator<Item = &ComponentAddress> + '_ {
        match self {
            Self::V1(tx) => tx.as_referenced_components(),
        }
    }

    /// Returns an iterator that iterates over all the statically referenced template addresses in this transaction.
    /// NOTE: This does not include templates required for component method calls.
    pub fn referenced_templates_iter(&self) -> impl Iterator<Item = &TemplateAddress> + '_ {
        self.instructions()
            .iter()
            .chain(self.fee_instructions())
            .filter_map(|instruction| {
                if let Instruction::CallFunction { address, .. } = instruction {
                    Some(address)
                } else {
                    None
                }
            })
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
    use tari_crypto::{
        keys::{PublicKey as _, SecretKey},
        ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    };
    use tari_engine_types::ToByteType;
    use tari_ootle_common_types::crypto::create_key_pair;
    use tari_template_lib::{prelude::Bytes, types::TemplateAddress};

    use super::*;
    use crate::{args, call_args};

    fn create_transaction() -> TransactionBuilder {
        Transaction::builder()
            .for_network(123u8)
            .create_account(Default::default())
            .call_method(ComponentAddress::from_array([1; 32]), "method", args![
                1,
                2,
                3,
                "string",
                call_args![1, 2],
                Bytes::from_vec(vec![12; 100])
            ])
            .call_function(TemplateAddress::from_array([1; 32]), "function", args![
                1,
                2,
                3,
                ComponentAddress::from_array([1; 32])
            ])
            .put_last_instruction_output_on_workspace("workspace")
            .publish_template(b"template".to_vec().try_into().unwrap())
            .add_input(SubstateRequirement::versioned(
                SubstateId::Component(ComponentAddress::from_array([1; 32])),
                1,
            ))
    }

    #[test]
    fn it_encodes_and_decodes_without_errors() {
        let (k, _) = create_key_pair();
        // This test simply checks that there are no serde tags used that can cause encoding/decoding issues with
        // tari_bor
        let subject = create_transaction().build_and_seal(&k);
        let encoded = tari_bor::encode(&subject).unwrap();
        let _decoded = tari_bor::decode::<Transaction>(&encoded).unwrap();
    }

    #[test]
    fn it_correctly_signs_and_verifies() {
        let secret = RistrettoSecretKey::random(&mut OsRng);
        let public_key = RistrettoPublicKey::from_secret_key(&secret);
        let subject = create_transaction().build_and_seal(&secret);
        assert!(subject.verify_all_signatures());

        let secret2 = RistrettoSecretKey::random(&mut OsRng);
        let subject = create_transaction()
            .add_signer(&public_key.to_byte_type(), &secret2)
            .build_and_seal(&secret);
        assert!(subject.verify_all_signatures());
    }
}
