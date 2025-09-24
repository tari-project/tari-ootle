//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[template]
mod template {
    use tari_template_lib::prelude::crypto::StealthValueProof;

    use super::*;

    pub struct StealthFaucet {
        manager: ResourceManager,
        supply_vault: Vault,
    }

    impl StealthFaucet {
        pub fn new(
            initial_supply: Amount,
            mint: StealthTransferStatement,
            view_key: Option<RistrettoPublicKeyBytes>,
        ) -> Component<Self> {
            let signer = NonFungibleAddress::from_public_key(CallerContext::transaction_signer_public_key());
            let bucket = ResourceBuilder::stealth()
                .mintable(rule!(allow_all))
                .freezable(rule!(non_fungible(signer)))
                .with_view_key_opt(view_key)
                .initial_supply(initial_supply);

            let resource_address = bucket.resource_address();
            // Convert the minted funds to UTXOs as per the stealth transfer.
            let revealed_output_bucket = bucket.stealth_transfer(mint);
            let supply_vault = Vault::from_bucket(revealed_output_bucket);

            Component::new(Self {
                manager: resource_address.into(),
                supply_vault,
            })
            .with_access_rules(AccessRules::allow_all())
            .create()
        }

        pub fn take_funds(&self, amount: Amount) -> Bucket {
            self.supply_vault.withdraw(amount)
        }

        pub fn programmatic_transfer(&self, transfer: StealthTransferStatement) {
            // You could check the output revealed amount before calling stealth transfer - however, this is not
            // strictly necessary because the transfer below will fail (returned bucket will be None) if the
            // revealed output amount is zero.
            //
            // if transfer.outputs_statement.revealed_output_amount <= Amount::zero() {
            //     panic!("Revealed output amount must be positive");
            // }

            // If there are any revealed inputs required, we'll take it from the supply vault.
            let maybe_input_bucket = if transfer.inputs_statement.revealed_amount.is_positive() {
                Some(self.supply_vault.withdraw(transfer.inputs_statement.revealed_amount))
            } else {
                None
            };

            let bucket = self
                .manager
                .stealth_transfer_with_opt_input_bucket(transfer, maybe_input_bucket)
                .expect("Stealth transfers must revealed output amounts (which we'll take for ourselves mwahaha!)");
            // All revealed funds are transferred to the component's vault.
            self.supply_vault.deposit(bucket);
        }

        pub fn mint(&self, amount: Amount) {
            let bucket = self.manager.mint_stealth(amount);
            self.supply_vault.deposit(bucket);
        }

        pub fn freeze_utxos(&self, utxos: Vec<UtxoId>) {
            self.manager.freeze_utxos(utxos);
        }

        pub fn unfreeze_utxos(&self, utxos: Vec<UtxoId>) {
            self.manager.unfreeze_utxos(utxos);
        }

        pub fn burn_utxos(&self, utxos: Vec<(UtxoId, StealthValueProof)>) {
            for (utxo, proof) in utxos {
                self.manager.burn_utxo(utxo, Some(proof));
            }
        }
    }
}
