//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::info;
use tari_ootle_common_types::SubstateRequirement;
use tari_ootle_transaction::args;
use tari_ootle_wallet_sdk::models::Account;
use tari_template_lib_types::{ComponentAddress, ResourceAddress, VaultId, constants::TARI_TOKEN};

use crate::runner::Runner;

pub struct Faucet {
    pub component_address: ComponentAddress,
    pub resource_address: ResourceAddress,
    pub vault_address: VaultId,
}

impl Runner {
    pub async fn create_faucet(&mut self, in_account: &Account) -> anyhow::Result<Faucet> {
        let key = self.sdk.key_manager_api().derive_account_key(0)?;

        let fee_vault = self
            .sdk
            .accounts_api()
            .get_vault_by_resource(&in_account.component_address, &TARI_TOKEN)?;

        let transaction = self
            .new_transaction_builder()
            .pay_fee_from_component(in_account.component_address, 1000u64)
            .call_function(self.faucet_template, "mint", args![1_000_000_000])
            .with_inputs([
                SubstateRequirement::unversioned(in_account.component_address),
                SubstateRequirement::unversioned(fee_vault.id),
                SubstateRequirement::unversioned(fee_vault.resource_address),
            ])
            .build_and_seal(&key.key);

        let finalize = self.submit_transaction_and_wait(transaction).await?;
        let diff = finalize.result.any_accept().unwrap();

        let component_address = diff
            .up_iter()
            .find_map(|(addr, s)| {
                addr.as_component_address()
                    .filter(|_| s.substate_value().component().unwrap().template_address == self.faucet_template)
            })
            .ok_or_else(|| anyhow::anyhow!("Faucet Component address not found"))?;
        let resource_address = diff
            .up_iter()
            .filter(|(addr, _)| *addr != TARI_TOKEN)
            .find_map(|(addr, _)| addr.as_resource_address())
            .ok_or_else(|| anyhow::anyhow!("Faucet Resource address not found"))?;
        let vault_address = diff
            .up_iter()
            .filter_map(|(addr, _)| addr.as_vault_id())
            .find(|addr| *addr != fee_vault.id)
            .ok_or_else(|| anyhow::anyhow!("Faucet Resource address not found"))?;

        info!("✅ Faucet {component_address} created with {resource_address} and {vault_address}");

        Ok(Faucet {
            component_address,
            resource_address,
            vault_address,
        })
    }
}
