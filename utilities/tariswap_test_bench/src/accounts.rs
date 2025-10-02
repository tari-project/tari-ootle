//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::RangeInclusive;

use log::info;
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey};
use tari_engine_types::{
    component::derive_component_address_from_public_key,
    indexed_value::IndexedWellKnownTypes,
    ToByteType,
};
use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_wallet_sdk::models::{Account, KeyId};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    constants::{XTR, XTR_FAUCET_COMPONENT_ADDRESS, XTR_FAUCET_VAULT_ADDRESS},
    prelude::ResourceType,
};
use tari_transaction::args;

use crate::{faucet::Faucet, runner::Runner};

impl Runner {
    pub async fn create_account_with_free_coins(&mut self) -> anyhow::Result<Account> {
        let key = self.sdk.key_manager_api().derive_account_key(0)?;
        let owner_public_key = RistrettoPublicKey::from_secret_key(&key.key).to_byte_type();

        let account_address = derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &owner_public_key);

        let transaction = self
            .new_transaction_builder()
            .with_fee_instructions_builder(|builder| {
                builder
                    .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![1_000_000_000])
                    .put_last_instruction_output_on_workspace("coins")
                    .create_account_with_bucket(owner_public_key, "coins")
                    .call_method(account_address, "pay_fee", args![1000])
            })
            .with_inputs([
                SubstateRequirement::unversioned(XTR_FAUCET_COMPONENT_ADDRESS),
                SubstateRequirement::unversioned(XTR_FAUCET_VAULT_ADDRESS),
            ])
            .build_and_seal(&key.key);

        let finalize = self.submit_transaction_and_wait(transaction).await?;
        let diff = finalize.result.any_accept().unwrap();
        let account = diff
            .up_iter()
            .find_map(|(addr, _)| addr.as_component_address())
            .unwrap();
        let vault = diff
            .up_iter()
            .filter_map(|(addr, _)| addr.as_vault_id())
            .find(|vault_id| *vault_id != XTR_FAUCET_VAULT_ADDRESS)
            .unwrap();

        self.sdk
            .accounts_api()
            .add_account(None, &account, KeyId::derived(0), KeyId::derived(0), true, true)?;
        self.sdk
            .accounts_api()
            .add_vault(account, vault, XTR, ResourceType::Stealth, Some("XTR".to_string()), 6)?;
        let account = self.sdk.accounts_api().get_account_by_address(&account)?;

        Ok(account.account)
    }

    pub async fn create_accounts(
        &mut self,
        pay_fee_account: &Account,
        account_key_indexes: RangeInclusive<u64>,
    ) -> anyhow::Result<Vec<Account>> {
        let key = self.sdk.key_manager_api().derive_account_key(0)?;
        let key_index_start = *account_key_indexes.start();
        let num_accounts = *account_key_indexes.end() as usize - key_index_start as usize + 1;
        let owners = account_key_indexes
            .map(|idx| {
                let key = self.sdk.key_manager_api().derive_account_key(idx)?;
                Ok(key)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let mut builder = self
            .new_transaction_builder()
            .fee_transaction_pay_from_component(pay_fee_account.component_address, 1000 * owners.len());
        for owner in &owners {
            builder = builder.create_account(RistrettoPublicKey::from_secret_key(&owner.key).to_byte_type());
        }

        let pay_fee_vault = self
            .sdk
            .accounts_api()
            .get_vault_by_resource(&pay_fee_account.component_address, &XTR)?;

        let transaction = builder
            .with_inputs([
                SubstateRequirement::unversioned(pay_fee_account.component_address),
                SubstateRequirement::unversioned(pay_fee_vault.id),
                SubstateRequirement::unversioned(pay_fee_vault.resource_address),
            ])
            .build_and_seal(&key.key);

        let finalize = self.submit_transaction_and_wait(transaction).await?;
        let diff = finalize.result.any_accept().unwrap();
        let mut accounts = Vec::with_capacity(num_accounts);

        for owner in owners {
            let account_addr = diff
                .up_iter()
                .map(|(addr, _)| addr)
                .filter_map(|addr| addr.as_component_address())
                .filter(|addr| *addr != pay_fee_account.component_address)
                .find(|addr| {
                    derive_component_address_from_public_key(
                        &ACCOUNT_TEMPLATE_ADDRESS,
                        &RistrettoPublicKey::from_secret_key(&owner.key).to_byte_type(),
                    ) == *addr
                })
                .expect("New account not found in diff");

            self.sdk.accounts_api().add_account(
                None,
                &account_addr,
                owner.as_key_id(),
                owner.as_key_id(),
                true,
                false,
            )?;
            let account = self.sdk.accounts_api().get_account_by_address(&account_addr)?;
            accounts.push(account.account);
        }

        Ok(accounts)
    }

    pub async fn fund_accounts(
        &mut self,
        faucet: &Faucet,
        fee_account: &Account,
        all_accounts: &[Account],
    ) -> anyhow::Result<()> {
        let key = self.sdk.key_manager_api().derive_account_key(0)?;
        let fee_vault = self
            .sdk
            .accounts_api()
            .get_vault_by_resource(&fee_account.component_address, &XTR)?;

        for accounts in all_accounts.chunks(100) {
            let transaction = self
                .new_transaction_builder()
                .fee_transaction_pay_from_component(fee_account.component_address, 1000 * accounts.len())
                .then(|builder| {
                    accounts.iter().fold(builder, |builder, account| {
                        builder
                            .call_method(faucet.component_address, "take_free_coins", args![])
                            .put_last_instruction_output_on_workspace("faucet")
                            .call_method(account.component_address, "deposit", args![Workspace("faucet")])
                            .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![1_000_000])
                            .put_last_instruction_output_on_workspace("xtr")
                            .call_method(account.component_address, "deposit", args![Workspace("xtr")])
                            .add_input(SubstateRequirement::unversioned(account.component_address))
                    })
                })
                .with_inputs([
                    SubstateRequirement::unversioned(XTR),
                    SubstateRequirement::unversioned(XTR_FAUCET_COMPONENT_ADDRESS),
                    SubstateRequirement::unversioned(XTR_FAUCET_VAULT_ADDRESS),
                    SubstateRequirement::unversioned(faucet.component_address),
                    SubstateRequirement::unversioned(faucet.resource_address),
                    SubstateRequirement::unversioned(faucet.vault_address),
                    SubstateRequirement::unversioned(fee_vault.account_address),
                    SubstateRequirement::unversioned(fee_vault.id),
                ])
                .build_and_seal(&key.key);

            log::debug!(
                "Submitted transaction {} to fund {} accounts",
                transaction.calculate_id(),
                accounts.len()
            );
            let result = self.submit_transaction_and_wait(transaction).await?;
            let accounts_and_state = result
                .result
                .any_accept()
                .unwrap()
                .up_iter()
                .filter(|(addr, _)| {
                    *addr != XTR_FAUCET_COMPONENT_ADDRESS &&
                        *addr != faucet.component_address &&
                        *addr != fee_account.component_address
                })
                .filter_map(|(addr, substate)| {
                    Some((addr.as_component_address()?, substate.substate_value().component()?))
                })
                .map(|(addr, component)| (addr, IndexedWellKnownTypes::from_value(&component.body.state).unwrap()));

            for (account, indexed) in accounts_and_state {
                log::debug!("Funded account {account} with vaults:");
                for vault_id in indexed.vault_ids() {
                    let vault = result
                        .result
                        .any_accept()
                        .unwrap()
                        .up_iter()
                        .find(|(addr, _)| addr == vault_id)
                        .map(|(_, substate)| substate.substate_value().vault().unwrap())
                        .unwrap_or_else(|| {
                            panic!("Vault {vault_id} not found in diff");
                        });
                    log::debug!("- {} {} {}", vault_id, vault.resource_address(), vault.resource_type());
                    self.sdk.accounts_api().add_vault(
                        account,
                        *vault_id,
                        *vault.resource_address(),
                        vault.resource_type(),
                        None,
                        0,
                    )?;
                }
            }
            info!("✅ Funded 100 accounts");
        }

        Ok(())
    }
}
