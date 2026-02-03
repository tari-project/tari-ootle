//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::SubstateRequirementRef;
use tari_ootle_transaction::{Transaction, TransactionId, TransactionWeight};
use tari_template_lib::prelude::RistrettoPublicKeyBytes;

use crate::executables::{Executable, Instructions, WeightedExecutable};

impl Executable for Transaction {
    fn to_id(&self) -> TransactionId {
        self.calculate_id()
    }

    fn all_inputs_iter(&self) -> impl Iterator<Item = SubstateRequirementRef<'_>> + '_ {
        self.all_inputs_iter()
    }

    fn signers_iter(&self) -> impl Iterator<Item = &RistrettoPublicKeyBytes> {
        Some(self.seal_signature().public_key())
            .filter(|_| self.is_seal_signer_authorized())
            .into_iter()
            .chain(self.signatures().iter().map(|s| s.public_key()))
    }

    fn into_instructions(self) -> Instructions {
        let (fee, main) = self.into_instruction_parts();
        Instructions { fee, main }
    }
}

impl WeightedExecutable for Transaction {
    fn calculate_weight(&self) -> TransactionWeight {
        self.calculate_transaction_weight()
    }
}
