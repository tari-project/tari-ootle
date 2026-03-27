//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod transaction;

use tari_ootle_common_types::SubstateRequirementRef;
use tari_ootle_transaction::{Instruction, TransactionId, TransactionWeight};
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;

pub struct Instructions {
    pub fee: Vec<Instruction>,
    pub main: Vec<Instruction>,
}

pub trait Executable {
    fn to_id(&self) -> TransactionId;

    fn all_inputs_iter(&self) -> impl Iterator<Item = SubstateRequirementRef<'_>> + '_;

    /// Returns the main signer of the executable, if any.
    fn main_signer(&self) -> Option<RistrettoPublicKeyBytes> {
        self.signers_iter().next().copied()
    }
    /// Returns an iterator over all signers of the executable including the main signer.
    fn signers_iter(&self) -> impl Iterator<Item = &RistrettoPublicKeyBytes>;

    fn into_instructions(self) -> Instructions;
}

pub trait WeightedExecutable: Executable {
    fn calculate_weight(&self) -> TransactionWeight;
}
