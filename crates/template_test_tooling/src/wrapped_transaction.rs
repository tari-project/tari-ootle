//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine::executables::{Executable, Instructions, WeightedExecutable};
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::SubstateRequirement;
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

pub struct WrappedTransaction {
    transaction: tari_ootle_transaction::Transaction,
    inputs: Vec<SubstateRequirement>,
}

impl WrappedTransaction {
    pub fn new(transaction: tari_ootle_transaction::Transaction) -> Self {
        Self {
            transaction,
            inputs: vec![],
        }
    }

    pub fn extend_inputs<I: IntoIterator<Item = SubstateRequirement>>(&mut self, inputs: I) -> &mut Self {
        self.inputs.extend(inputs);
        self
    }
}

impl Executable for WrappedTransaction {
    fn to_id(&self) -> tari_ootle_transaction::TransactionId {
        self.transaction.calculate_id()
    }

    fn all_inputs_iter(&self) -> impl Iterator<Item = SubstateId> + '_ {
        // Combine the inputs from the transaction and the additional inputs
        // Note: duplicates are possible
        self.transaction
            .all_inputs_iter()
            .chain(self.inputs.iter().map(|id| id.as_ref()))
            .map(|req| req.substate_id().clone())
    }

    fn main_signer(&self) -> Option<RistrettoPublicKeyBytes> {
        self.transaction.main_signer()
    }

    fn signers_iter(&self) -> impl Iterator<Item = &RistrettoPublicKeyBytes> {
        self.transaction.signers_iter()
    }

    fn into_instructions(self) -> Instructions {
        self.transaction.into_instructions()
    }
}

impl WeightedExecutable for WrappedTransaction {
    fn calculate_weight(&self) -> tari_ootle_transaction::TransactionWeight {
        self.transaction.calculate_weight()
    }
}
