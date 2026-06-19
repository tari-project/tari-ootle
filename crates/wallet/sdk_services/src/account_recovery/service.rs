// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use log::*;
use ootle_byte_type::ToByteType;
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_engine_types::component::derive_component_address_from_public_key;
use tari_ootle_common_types::{
    Epoch,
    displayable::Displayable,
    optional::{IsNotFoundError, Optional},
    response_status::TransactionStatusResponseError,
    substate_type::SubstateType,
};
use tari_ootle_wallet_sdk::{
    WalletSdk,
    WalletSdkSpec,
    apis::config::ConfigKey,
    models::{DerivedWalletKey, KeyBranch, KeyId},
    network::WalletNetworkInterface,
};
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tokio::time;

use crate::{account_monitor::AccountMonitorHandle, account_recovery::AccountRecoveryError};

const LOG_TARGET: &str = "tari::ootle_wallet_daemon::resource_scanner";

/// Scans through all the substates to find related resources to current wallet.
pub struct AccountRecoveryService<TSpec: WalletSdkSpec> {
    wallet_sdk: WalletSdk<TSpec>,
    account_monitor_handle: AccountMonitorHandle,
    abandon_after_not_found: usize,
    cipher_seed_birthday_epoch: Epoch,
}

impl<TSpec> AccountRecoveryService<TSpec>
where
    TSpec: WalletSdkSpec,
    <TSpec::NetworkInterface as WalletNetworkInterface>::Error: IsNotFoundError + TransactionStatusResponseError,
{
    pub fn new(
        wallet_sdk: WalletSdk<TSpec>,
        account_monitor_handle: AccountMonitorHandle,
        abandon_after_not_found: usize,
        cipher_seed_birthday_epoch: Epoch,
    ) -> Self {
        Self {
            wallet_sdk,
            account_monitor_handle,
            abandon_after_not_found,
            cipher_seed_birthday_epoch,
        }
    }

    pub async fn scan(self) {
        info!(target: LOG_TARGET, "Waiting for indexer to be ready...");

        // wait for indexer to be ready
        loop {
            match self.wallet_sdk.get_network_interface().wait_until_ready().await {
                Ok(()) => {
                    break;
                },
                Err(error) => {
                    warn!(target: LOG_TARGET, "Indexer is not ready yet: {error:?}");
                },
            }
            time::sleep(Duration::from_secs(5)).await;
        }

        info!(target: LOG_TARGET, "🔑 Attempting to recover accounts...");

        // if we don't find 10 accounts in a row just stop scanning
        let key_manager_api = self.wallet_sdk.key_manager_api();
        let mut not_found_accounts_count = 0;
        let mut found_accounts_count = 0;
        let initial_key_index = match key_manager_api.get_active_key(KeyBranch::Account) {
            Ok(key) => key.key_index(),
            Err(err) => {
                error!(target: LOG_TARGET, "Error getting active key: {err}. Scanning failed...");
                return;
            },
        };
        let mut last_found_key = None;
        loop {
            let key = match key_manager_api.next_key(KeyBranch::Account) {
                Ok(key) => key,
                Err(err) => {
                    error!(target: LOG_TARGET, "Error getting next key: {err}. Scanning failed...");
                    return;
                },
            };
            info!(target: LOG_TARGET, "🔍️ Attempting to recover account with key index {}", key.key_index());
            match self.try_recover_account(&key).await {
                Ok(true) => {
                    last_found_key = Some(key.key_index());
                    info!(target: LOG_TARGET, "✅ Account with key index {} found!", key.key_index());
                    not_found_accounts_count = 0;
                    found_accounts_count += 1;
                },
                Ok(false) => {
                    not_found_accounts_count += 1;
                },
                Err(err) => {
                    warn!(target: LOG_TARGET, "⚠️Error scanning account: {err}. Ignoring and continuing...");
                    not_found_accounts_count += 1;
                },
            }
            if not_found_accounts_count == self.abandon_after_not_found {
                info!(
                    target: LOG_TARGET,
                    "No accounts found for {} consecutive keys. Assuming that we are done", self.abandon_after_not_found
                );
                break;
            }
        }

        if let Some(last_found_key) = last_found_key {
            info!(target: LOG_TARGET, "Setting active key to {}", last_found_key);
            if let Err(err) = key_manager_api.reset_key_index_to(KeyBranch::Account, last_found_key) {
                error!(target: LOG_TARGET, "Error setting active key: {err}");
            }
            if let Err(err) = key_manager_api.set_active_key(KeyBranch::Account, last_found_key) {
                error!(target: LOG_TARGET, "Error setting active key: {err}");
            }
        } else {
            info!(target: LOG_TARGET, "No accounts found. Setting active key to {}", initial_key_index);
            if let Err(err) = key_manager_api.reset_key_index_to(KeyBranch::Account, initial_key_index) {
                error!(target: LOG_TARGET, "Error setting active key: {err}");
            }
            if let Err(err) = key_manager_api.set_active_key(KeyBranch::Account, initial_key_index) {
                error!(target: LOG_TARGET, "Error setting active key: {err}");
            }
        }

        // Set a flag to indicate that the wallet has completed recovery
        if let Err(err) = self.wallet_sdk.config_api().set(ConfigKey::RecoveryNeeded, &false) {
            error!(target: LOG_TARGET, "Error setting recovery needed flag: {err}");
        }

        info!(target: LOG_TARGET, "✅ Scanning accounts finished! {found_accounts_count} owned account(s) found!");
    }

    /// Attempt to recover an account by the provided public key. Returning true if the account was found on-chain,
    /// false if not.
    async fn try_recover_account(&self, key: &DerivedWalletKey) -> Result<bool, AccountRecoveryError> {
        let network_interface = self.wallet_sdk.get_network_interface();

        let public_key = RistrettoPublicKey::from_secret_key(&key.key).to_byte_type();
        // Derive the account address from the public key
        let account_addr = derive_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &public_key);

        // Attempt to fetch the account substate
        // TODO: perf we should batch these requests e.g. query 10 accounts in one go
        let result = network_interface
            .query_substate(&account_addr.into(), None, false)
            .await
            .optional()
            .map_err(|e| AccountRecoveryError::NetworkInterfaceError { details: e.to_string() })?;

        // We use the cipher seed birthday as the account birthday since that is simpler than attempting to fetch the
        // creation epoch for each account.
        let birthday_epoch = self.cipher_seed_birthday_epoch;

        match result {
            None => {
                info!(target: LOG_TARGET, "🔑 Account {} not found on chain. It may have stealth UTXOs owned by its key", account_addr);

                // We cannot find this account on chain, however there could be UTXOs owned by this key which we'll need
                // to scan for.
                self.wallet_sdk.accounts_api().add_account(
                    Some(format!("recovered-account-{}", key.key_index()).as_str()),
                    &account_addr,
                    KeyId::derived(KeyBranch::ViewOnlyKey, key.key_index()),
                    key.as_key_id(),
                    birthday_epoch,
                    false,
                    // if this is the first account, set it as the default
                    key.key_index() == 0,
                )?;

                // Update UTXOs
                self.account_monitor_handle
                    .recover_account(account_addr)
                    .await?;
                // Count this as not found for the purposes of stopping the scan after N not founds
                Ok(false)
            },
            Some(result) => {
                let component =
                    result
                        .substate
                        .as_component()
                        .ok_or_else(|| AccountRecoveryError::InvalidResponse {
                            details: format!(
                                "Expected component substate for address {account_addr}, got {}",
                                SubstateType::from(&result.substate)
                            ),
                        })?;

                if component.owner_public_key().is_none() {
                    warn!(target: LOG_TARGET, "⚠️ Account {} has no owner key. This wallet may not be able to use this account", account_addr);
                };

                if component.owner_public_key().is_some_and(|pk| *pk != public_key) {
                    warn!(
                        target: LOG_TARGET,
                        "⚠️ Account {} has a different owner key {} than the one derived from the seed key {}. This wallet may not be able to use this account",
                        account_addr,
                        component.owner_public_key().display(),
                        public_key
                    );
                };

                // add account
                info!(
                    target: LOG_TARGET,
                    "🔑 Adding account {} with owner key {} and key index {}",
                    account_addr,
                    component.owner_public_key().display(),
                    key.key_index()
                );
                self.wallet_sdk.accounts_api().add_account(
                    Some(format!("recovered-account-{}", key.key_index()).as_str()),
                    &account_addr,
                    KeyId::derived(KeyBranch::ViewOnlyKey, key.key_index()),
                    KeyId::derived(KeyBranch::Account, key.key_index()),
                    birthday_epoch,
                    true,
                    // if this is the first account, set it as the default
                    key.key_index() == 0,
                )?;

                // Update vaults, UTXOs, nfts etc
                self.account_monitor_handle
                    .recover_account(account_addr)
                    .await?;

                Ok(true)
            },
        }
    }
}
