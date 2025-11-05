//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    use super::*;

    pub struct XtrFaucet {
        vault: Vault,
    }

    impl XtrFaucet {
        pub fn take(&self, amount: Amount) -> Bucket {
            debug!("Withdrawing {} coins from faucet", amount);
            let signer = CallerContext::transaction_signer_public_key();
            emit_event("take", [("amount", amount.to_string()), ("signer", signer.to_string())]);
            self.vault.withdraw(amount)
        }

        pub fn take_confidential(
            &self,
            amount: Amount,
            output: StealthOutputsStatement,
            balance_proof: Option<BalanceProofSignature>,
        ) -> Option<Bucket> {
            let signer = CallerContext::transaction_signer_public_key();
            let revealed_bucket = self.vault.withdraw(amount);
            let transfer = StealthTransferStatement {
                inputs_statement: StealthInputsStatement::new_revealed_only(amount, signer),
                outputs_statement: output,
                balance_proof,
            };

            debug!("Withdrawing {} coins from faucet into confidential output", amount);
            emit_event("take", [
                ("amount", amount.to_string()),
                ("confidential", "true".to_string()),
                ("signer", signer.to_string()),
            ]);

            self.vault
                .to_resource_manager()
                .stealth_transfer_with_opt_input_bucket(transfer, Some(revealed_bucket))
        }
    }
}
