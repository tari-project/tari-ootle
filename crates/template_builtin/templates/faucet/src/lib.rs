//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::{prelude::*, types::constants::XTR_FAUCET_CLAIM_RESOURCE_ADDRESS};

#[template]
mod template {
    /// Faucet amount in microtari: 1,000 TARI per claim. Fixed for take(), ceiling for take_confidential().
    const FAUCET_AMOUNT: u64 = 1_000 * 1_000_000;
    use super::*;

    pub struct XtrFaucet {
        vault: Vault,
    }

    impl XtrFaucet {
        /// Mints a claim-receipt NFT keyed to the caller's public key, then immediately burns it.
        /// The burned substate key persists on-chain, so a second attempt panics with DuplicateNonFungibleId.
        fn record_claim(&self) {
            let pk = CallerContext::transaction_signer_public_key();
            let receipt = ResourceManager::get(XTR_FAUCET_CLAIM_RESOURCE_ADDRESS)
                .mint_non_fungible(NonFungibleId::from_public_key(pk), &(), &());
            receipt.burn();
        }

        /// Gives exactly 1,000 TARI to the caller. Can only be called once per signing public key.
        pub fn take(&self) -> Bucket {
            self.record_claim();
            emit_event("take", metadata!["amount" => FAUCET_AMOUNT.to_string()]);
            self.vault.withdraw(FAUCET_AMOUNT)
        }

        pub fn take_confidential(&self, transfer: StealthTransferStatement) -> Option<Bucket> {
            let amount = transfer.inputs_statement.revealed_amount;
            assert!(
                amount <= FAUCET_AMOUNT,
                "Requested amount {} exceeds faucet max of {}",
                amount,
                FAUCET_AMOUNT
            );

            self.record_claim();
            emit_event("take", metadata![
                "amount" => amount.to_string(),
                "confidential" => "true".to_string()
            ]);

            // The faucet adds the revealed amount to the transfer
            let revealed_bucket = self.vault.withdraw(amount);
            self.vault
                .get_resource_manager()
                .stealth_transfer_with_opt_input_bucket(transfer, Some(revealed_bucket))
        }
    }
}
