//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    const FAUCET_MAX: u64 = 10_000 * 1_000_000;
    use super::*;

    pub struct XtrFaucet {
        vault: Vault,
    }

    impl XtrFaucet {
        pub fn take(&self, amount: Amount) -> Bucket {
            assert!(
                amount <= FAUCET_MAX,
                "Requested amount {} exceeds faucet max of {}",
                amount,
                FAUCET_MAX
            );
            emit_event("take", metadata!["amount" => amount.to_string()]);
            self.vault.withdraw(amount)
        }

        pub fn take_confidential(
            &self,
            amount: Amount,
            output: StealthOutputsStatement,
            balance_proof: Option<BalanceProofSignature>,
        ) -> Option<Bucket> {
            assert!(
                amount <= FAUCET_MAX,
                "Requested amount {} exceeds faucet max of {}",
                amount,
                FAUCET_MAX
            );
            let revealed_bucket = self.vault.withdraw(amount);
            let transfer = StealthTransferStatement {
                inputs_statement: StealthInputsStatement::new_revealed_only(amount),
                outputs_statement: output,
                balance_proof,
            };

            emit_event("take", metadata![
                "amount" => amount.to_string(),
                "confidential" => "true".to_string()
            ]);

            self.vault
                .to_resource_manager()
                .stealth_transfer_with_opt_input_bucket(transfer, Some(revealed_bucket))
        }
    }
}
