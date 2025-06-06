// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{collections::HashMap, time::Duration};

use log::{error, info, warn};
use tari_crypto::{keys::PublicKey, ristretto::RistrettoPublicKey};
use tari_engine_types::{component::new_component_address_from_public_key, ToByteType};
use tari_key_manager::key_manager::DerivedKey;
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    substate_type::SubstateType,
};
use tari_ootle_wallet_sdk::{
    apis::{
        accounts::AccountsApiError,
        config::ConfigKey,
        key_manager::{KeyManagerApiError, TRANSACTION_BRANCH},
    },
    network::WalletNetworkInterface,
    storage::{WalletStorageError, WalletStore},
    WalletSdk,
};
use tari_shutdown::ShutdownSignal;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::prelude::RistrettoPublicKeyBytes;

use crate::services::{account_monitor::AccountMonitorError, AccountMonitorHandle};

const LOG_TARGET: &str = "tari::ootle_wallet_daemon::resource_scanner";

/// Scans through all the substates to find related resources to current wallet.
pub struct Service<TStore, TNetworkInterface> {
    wallet_sdk: WalletSdk<TStore, TNetworkInterface>,
    account_monitor_handle: AccountMonitorHandle,
    abandon_after_not_found: usize,
    shutdown_signal: ShutdownSignal,
}

impl<TStore, TNetworkInterface> Service<TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError,
{
    pub fn new(
        wallet_sdk: WalletSdk<TStore, TNetworkInterface>,
        account_monitor_handle: AccountMonitorHandle,
        abandon_after_not_found: usize,
        shutdown_signal: ShutdownSignal,
    ) -> Self {
        Self {
            wallet_sdk,
            account_monitor_handle,
            abandon_after_not_found,
            shutdown_signal,
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
            if self.shutdown_signal.is_triggered() {
                info!(target: LOG_TARGET, "Shutdown signal received. Stopping scan.");
                break;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        if self.shutdown_signal.is_triggered() {
            info!(target: LOG_TARGET, "Shutdown signal received. Stopping scan.");
            return;
        }

        info!(target: LOG_TARGET, "🔑 Attempting to recover accounts...");

        // if we don't find 10 accounts in a row just stop scanning
        let key_manager_api = self.wallet_sdk.key_manager_api();
        let mut not_found_accounts_count = 0;
        let mut found_accounts_count = 0;
        let mut owner_key_cache = HashMap::new();
        let initial_key_index = match key_manager_api.get_active_key(TRANSACTION_BRANCH) {
            Ok((key_index, _)) => key_index,
            Err(err) => {
                error!(target: LOG_TARGET, "Error getting active key: {err}. Scanning failed...");
                return;
            },
        };
        let mut last_found_key = None;
        loop {
            let key = match key_manager_api.next_key(TRANSACTION_BRANCH) {
                Ok(key) => key,
                Err(err) => {
                    error!(target: LOG_TARGET, "Error getting next key: {err}. Scanning failed...");
                    return;
                },
            };
            info!(target: LOG_TARGET, "🔍️ Attempting to recover account with key index {}", key.key_index);
            match self.try_recover_account(&key, &mut owner_key_cache).await.optional() {
                Ok(Some(())) => {
                    last_found_key = Some(key.key_index);
                    info!(target: LOG_TARGET, "✅ Account with key index {} found!", key.key_index);
                    not_found_accounts_count = 0;
                    found_accounts_count += 1;
                },
                Ok(None) => {
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
            if let Err(err) = key_manager_api.reset_key_index_to(TRANSACTION_BRANCH, last_found_key) {
                error!(target: LOG_TARGET, "Error setting active key: {err}");
            }
            if let Err(err) = key_manager_api.set_active_key(TRANSACTION_BRANCH, last_found_key) {
                error!(target: LOG_TARGET, "Error setting active key: {err}");
            }
        } else {
            info!(target: LOG_TARGET, "No accounts found. Setting active key to {}", initial_key_index);
            if let Err(err) = key_manager_api.reset_key_index_to(TRANSACTION_BRANCH, initial_key_index) {
                error!(target: LOG_TARGET, "Error setting active key: {err}");
            }
            if let Err(err) = key_manager_api.set_active_key(TRANSACTION_BRANCH, initial_key_index) {
                error!(target: LOG_TARGET, "Error setting active key: {err}");
            }
        }

        // Set a flag to indicate that the wallet has completed recovery
        if let Err(err) = self
            .wallet_sdk
            .config_api()
            .set(ConfigKey::RecoveryNeeded, &false, false)
        {
            error!(target: LOG_TARGET, "Error setting recovery needed flag: {err}");
        }

        info!(target: LOG_TARGET, "✅ Scanning accounts finished! {found_accounts_count} owned account(s) found!");
    }

    /// Attempt to recover an account by the provided public key.
    async fn try_recover_account(
        &self,
        key: &DerivedKey<RistrettoPublicKey>,
        owner_key_cache: &mut HashMap<RistrettoPublicKeyBytes, u64>,
    ) -> Result<(), AccountScannerError> {
        let network_interface = self.wallet_sdk.get_network_interface();
        let accounts_api = self.wallet_sdk.accounts_api();

        let public_key = RistrettoPublicKey::from_secret_key(&key.key).to_byte_type();
        // Derive the account address from the public key
        let account_addr = new_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &public_key);

        // Attempt to fetch the account substate
        // TODO: perf we should batch these requests e.g. query 10 accounts in one go
        let result = network_interface
            .query_substate(&account_addr.into(), None, false)
            .await
            .optional()
            .map_err(|e| AccountScannerError::NetworkInterfaceError { details: e.to_string() })?
            .ok_or(AccountScannerError::AccountNotFound)?;

        let component = result
            .substate
            .as_component()
            .ok_or_else(|| AccountScannerError::InvariantError {
                details: format!(
                    "Expected component substate for address {account_addr}, got {}",
                    SubstateType::from(&result.substate)
                ),
            })?;

        let Some(owner_public_key) = component.owner_key else {
            info!(target: LOG_TARGET, "Account {} has no owner key", account_addr);
            return Ok(());
        };

        let key_index = component
            .owner_key
            .and_then(|owner_public_key| {
                if public_key == owner_public_key {
                    // Cache this in case we have another account that uses this key
                    owner_key_cache.insert(owner_public_key, key.key_index);
                    Some(key.key_index)
                } else {
                    Some(*owner_key_cache.get(&owner_public_key)?)
                }
            })
            .unwrap_or_else(|| {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ Account {} has a different owner key {} than the one derived from the seed key {}. Signing a transaction with this key index may not work.",
                    account_addr,
                    owner_public_key,
                    public_key
                );

                // TODO: this key index may not actually be the correct owner key - there is no guaranteed way to
                // find the key index from the public key. We'll need to find another way to do
                // this.
                key.key_index
            });

        // add account
        let has_default = accounts_api.get_default().optional()?.is_some();
        self.wallet_sdk.accounts_api().add_account(
            Some(format!("account-{}", key.key_index).as_str()),
            &account_addr.into(),
            key_index,
            !has_default,
        )?;

        // Update vaults, confidential outputs, nfts etc
        self.account_monitor_handle.refresh_account(account_addr.into()).await?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AccountScannerError {
    #[error("Wallet store error: {0}")]
    WalletStoreError(#[from] WalletStorageError),
    #[error("Key manager API error: {0}")]
    KeyManagerApiError(#[from] KeyManagerApiError),
    #[error("Accounts API error: {0}")]
    AccountsApiError(#[from] AccountsApiError),
    #[error("Network interface error: {details}")]
    NetworkInterfaceError { details: String },
    #[error("Account not found")]
    AccountNotFound,
    #[error("Account monitor error: {0}")]
    AccountMonitorError(#[from] AccountMonitorError),
    #[error("Invariant error: {details}")]
    InvariantError { details: String },
}

impl IsNotFoundError for AccountScannerError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, AccountScannerError::AccountNotFound)
    }
}
