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
            assert!(amount.is_positive(), "Amount must be positive");
            assert!(
                amount <= FAUCET_MAX,
                "Requested amount {} exceeds faucet max of {}",
                amount,
                FAUCET_MAX
            );
            emit_event("take", metadata!["amount" => amount.to_string()]);
            self.vault.withdraw(amount)
        }

        pub fn take_confidential(&self, transfer: StealthTransferStatement) -> Option<Bucket> {
            let amount = transfer.inputs_statement.revealed_amount;
            assert!(amount.is_positive(), "Amount must be positive");
            assert!(
                amount <= FAUCET_MAX,
                "Requested amount {} exceeds faucet max of {}",
                amount,
                FAUCET_MAX
            );

            emit_event("take", metadata![
                "amount" => amount.to_string(),
                "confidential" => "true".to_string()
            ]);

            // The faucet adds the revealed amount to the transfer
            let revealed_bucket = self.vault.withdraw(amount);
            self.vault
                .to_resource_manager()
                .stealth_transfer_with_opt_input_bucket(transfer, Some(revealed_bucket))
        }
    }
}
