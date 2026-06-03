//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::RangeInclusive;

use log::info;
use ootle_byte_type::ToByteType;
use tari_engine_types::indexed_value::IndexedWellKnownTypes;
use tari_ootle_common_types::{Epoch, SubstateRequirement};
use tari_ootle_transaction::args;
use tari_ootle_wallet_sdk::{
    apis::accounts::derive_account_address_from_public_key,
    models::{Account, KeyBranch, KeyId},
};
use tari_template_lib_types::{
    Amount,
    ResourceType,
    constants::{
        TARI_TOKEN,
        XTR_FAUCET_CLAIM_RESOURCE_ADDRESS,
        XTR_FAUCET_COMPONENT_ADDRESS,
        XTR_FAUCET_VAULT_ADDRESS,
    },
};

use crate::{faucet::Faucet, runner::Runner};

impl Runner {
    pub async fn create_account_with_free_coins(&mut self) -> anyhow::Result<Account> {
        let owner_key = self
            .sdk
            .key_manager_api()
            .get_public_key(KeyId::derived(KeyBranch::Account, 0))?;
        let owner_public_key = owner_key.public_key.to_byte_type();

        let transaction = self
            .new_transaction_builder()
            .with_fee_instructions_builder(|builder| {
                builder
                    .create_account(owner_public_key)
                    .put_last_instruction_output_on_workspace("account")
                    .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![Workspace("account")])
                    .pay_fee_from_component("account", 2000u64)
            })
            .with_inputs([
                SubstateRequirement::unversioned(XTR_FAUCET_COMPONENT_ADDRESS),
                SubstateRequirement::unversioned(XTR_FAUCET_VAULT_ADDRESS),
                SubstateRequirement::unversioned(XTR_FAUCET_CLAIM_RESOURCE_ADDRESS),
            ])
            .finish();

        let transaction = self.sdk.signer_api().sign(owner_key.key_id(), transaction)?;

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

        self.sdk.accounts_api().add_account(
            None,
            &account,
            KeyId::derived(KeyBranch::ViewOnlyKey, 0),
            KeyId::derived(KeyBranch::Account, 0),
            Epoch::zero(),
            true,
            true,
        )?;
        self.sdk.accounts_api().add_vault(
            account,
            vault,
            TARI_TOKEN,
            ResourceType::Stealth,
            Some("tTARI".to_string()),
            6,
        )?;
        let account = self.sdk.accounts_api().get_account_by_address(&account)?;

        Ok(account.account)
    }

    /// Creates accounts and funds each with XTR faucet tokens.
    /// Each account is created in its own transaction (signed by its owner key) so that the
    /// faucet's one-claim-per-signer limit is respected. Transactions are submitted concurrently
    /// and then waited on.
    pub async fn create_accounts(&mut self, account_key_indexes: RangeInclusive<u64>) -> anyhow::Result<Vec<Account>> {
        let num_accounts = *account_key_indexes.end() as usize - *account_key_indexes.start() as usize + 1;
        let owners = account_key_indexes
            .map(|idx| {
                let key = self
                    .sdk
                    .key_manager_api()
                    .get_public_key(KeyId::derived(KeyBranch::Account, idx))?;
                let account_key = self.sdk.key_manager_api().derive_account_key(idx)?;
                Ok((key, account_key))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let default_account = self.sdk.accounts_api().get_default()?;
        let fee_vault = self
            .sdk
            .accounts_api()
            .get_vault_by_resource(default_account.component_address(), &TARI_TOKEN)?;
        let default_acc_secret = self
            .sdk
            .key_manager_api()
            .get_key(default_account.owner_key_id().expect("default acc no owner key"))?;

        let transaction = self
            .new_transaction_builder()
            .fold(owners.iter().enumerate(), |builder, (i, (account, _))| {
                let component = format!("account_{i}");
                builder
                    .create_account(account.public_key().to_byte_type())
                    .put_last_instruction_output_on_workspace(&component)
                    .call_method(XTR_FAUCET_COMPONENT_ADDRESS, "take", args![Workspace(component)])
            })
            .pay_fee_from_component(
                *default_account.component_address(),
                Amount::from(1000 * num_accounts as u64),
            )
            .with_inputs([
                SubstateRequirement::unversioned(*default_account.component_address()),
                SubstateRequirement::unversioned(fee_vault.id),
                SubstateRequirement::unversioned(XTR_FAUCET_COMPONENT_ADDRESS),
                SubstateRequirement::unversioned(XTR_FAUCET_VAULT_ADDRESS),
                SubstateRequirement::unversioned(XTR_FAUCET_CLAIM_RESOURCE_ADDRESS),
            ])
            .build_and_seal(default_acc_secret.secret());

        let finalize = self.submit_transaction_and_wait(transaction).await?;
        let diff = finalize.result.any_accept().unwrap();

        let account_addrs = diff.up_iter().filter_map(|(addr, substate)| {
            // up_iter also yields non-owner components (the fee-paying account, the faucet), so skip
            // anything that isn't one of our newly created owner accounts rather than panicking.
            let addr = addr.as_component_address()?;
            let key_id = owners
                .iter()
                .find(|(pk, _)| addr == derive_account_address_from_public_key(&pk.public_key().to_byte_type()))
                .map(|(pk, _)| pk.key_id)?;
            let component = substate.substate_value().component()?;
            let indexed = component.body().to_indexed_well_known_types().ok()?;
            let vault_id = indexed
                .vault_ids()
                .iter()
                .find(|id| **id != XTR_FAUCET_VAULT_ADDRESS)
                .copied()?;

            Some((addr, vault_id, key_id))
        });

        let mut accounts = Vec::with_capacity(num_accounts);
        for (account_addr, vault_id, key_id) in account_addrs {
            self.sdk
                .accounts_api()
                .add_account(None, &account_addr, key_id, key_id, Epoch::zero(), true, false)?;
            self.sdk.accounts_api().add_vault(
                account_addr,
                vault_id,
                TARI_TOKEN,
                ResourceType::Stealth,
                Some("tTARI".to_string()),
                6,
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
            .get_vault_by_resource(&fee_account.component_address, &TARI_TOKEN)?;

        for accounts in all_accounts.chunks(25) {
            let transaction = self
                .new_transaction_builder()
                .pay_fee_from_component(
                    fee_account.component_address,
                    Amount::ONE_THOUSAND * Amount::from_usize(accounts.len()),
                )
                .fold(accounts.iter(), |builder, account| {
                    builder
                        .call_method(faucet.component_address, "take_free_coins", args![])
                        .put_last_instruction_output_on_workspace("faucet")
                        .call_method(account.component_address, "deposit", args![Workspace("faucet")])
                        .add_input(SubstateRequirement::unversioned(account.component_address))
                })
                .with_inputs([
                    SubstateRequirement::unversioned(faucet.component_address),
                    SubstateRequirement::unversioned(faucet.resource_address),
                    SubstateRequirement::unversioned(faucet.vault_address),
                    SubstateRequirement::unversioned(fee_vault.account_address),
                    SubstateRequirement::unversioned(fee_vault.id),
                ])
                .build_and_seal(&key.key);

            log::debug!(
                "Submitted transaction {} to fund {} accounts with custom faucet tokens",
                transaction.calculate_id(),
                accounts.len()
            );
            let result = self.submit_transaction_and_wait(transaction).await?;
            let accounts_and_state = result
                .result
                .any_accept()
                .unwrap()
                .up_iter()
                .filter(|(addr, _)| *addr != faucet.component_address && *addr != fee_account.component_address)
                .filter_map(|(addr, substate)| {
                    Some((addr.as_component_address()?, substate.substate_value().component()?))
                })
                .map(|(addr, component)| (addr, IndexedWellKnownTypes::from_value(component.state()).unwrap()));

            for (account, indexed) in accounts_and_state {
                log::debug!("Funded account {account} with vaults:");
                for vault_id in indexed.vault_ids() {
                    let Some(vault) = result
                        .result
                        .any_accept()
                        .unwrap()
                        .up_iter()
                        .find(|(addr, _)| addr == vault_id)
                        .map(|(_, substate)| substate.substate_value().vault().unwrap())
                    else {
                        // This vault is for another resource that was not used in this transaction
                        continue;
                    };
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
            info!("✅ Funded {} accounts with custom faucet tokens", accounts.len());
        }

        Ok(())
    }
}
