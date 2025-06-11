//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

mod transaction;

use tari_engine_types::instruction::Instruction;
use tari_ootle_common_types::SubstateRequirementRef;
use tari_template_lib::prelude::RistrettoPublicKeyBytes;
use tari_transaction::{TransactionId, TransactionWeight};

pub struct Instructions {
    pub fee: Vec<Instruction>,
    pub main: Vec<Instruction>,
}

pub trait Executable {
    fn to_id(&self) -> TransactionId;

    fn all_inputs_iter(&self) -> impl Iterator<Item = SubstateRequirementRef<'_>> + '_;

    fn main_signer(&self) -> Option<RistrettoPublicKeyBytes>;

    fn into_instructions(self) -> Instructions;
}

pub trait WeightedExecutable: Executable {
    fn calculate_weight(&self) -> TransactionWeight;
}
