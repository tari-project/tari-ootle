// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use log::{error, info, warn};
use tari_common_types::types::PublicKey;
use tari_crypto::tari_utilities::ByteArray;
use tari_dan_common_types::{optional::IsNotFoundError, VersionedSubstateId};
use tari_dan_wallet_sdk::{
    apis::{accounts::AccountsApiError, key_manager::TRANSACTION_BRANCH},
    network::WalletNetworkInterface,
    storage::{WalletStorageError, WalletStore},
    DanWalletSdk,
};
use tari_engine_types::{component::new_component_address_from_public_key, substate::SubstateId};
use tari_shutdown::ShutdownSignal;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;

const LOG_TARGET: &str = "tari::dan_wallet_daemon::resource_scanner";

/// Scans through all the substates to find related resources to current wallet.
pub struct Service<TStore, TNetworkInterface> {
    wallet_sdk: DanWalletSdk<TStore, TNetworkInterface>,
    shutdown_signal: ShutdownSignal,
}

impl<TStore, TNetworkInterface> Service<TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError,
{
    pub fn new(wallet_sdk: DanWalletSdk<TStore, TNetworkInterface>, shutdown_signal: ShutdownSignal) -> Self {
        Self {
            wallet_sdk,
            shutdown_signal,
        }
    }

    /// Checking for an account by its substate id
    async fn check_account(&self, substate_id: &SubstateId) -> bool {
        let network_interface = self.wallet_sdk.get_network_interface();
        match network_interface.query_substate(substate_id, None, false).await {
            Ok(result) => {
                if let Some(component) = result.substate.as_component() {
                    if let Some(owner_public_key) = component.owner_key {
                        match PublicKey::from_canonical_bytes(owner_public_key.as_bytes()) {
                            Ok(public_key) => {
                                let key_manager_api = self.wallet_sdk.key_manager_api();
                                if let Ok((index, _)) = key_manager_api.get_key_for_public_key_with_max_index(
                                    TRANSACTION_BRANCH,
                                    &public_key,
                                    u64::MAX,
                                ) {
                                    // add key index to DB
                                    if let Ok(last_index) =
                                        self.wallet_sdk.key_manager_api().last_index(TRANSACTION_BRANCH)
                                    {
                                        for _ in last_index + 1..=index {
                                            if let Err(error) =
                                                self.wallet_sdk.key_manager_api().next_key(TRANSACTION_BRANCH)
                                            {
                                                error!(target: LOG_TARGET, "Failed to add key to DB: {}", error);
                                            }
                                        }
                                    }

                                    // add account
                                    if let Err(error) = self.wallet_sdk.accounts_api().add_account(
                                        Some(format!("account-{}", index).as_str()),
                                        substate_id,
                                        index,
                                        false,
                                    ) {
                                        error!(target: LOG_TARGET, "Failed to add account: {:?}", error);
                                    }

                                    // set default account if not already set
                                    if let Err(AccountsApiError::StoreError(WalletStorageError::NotFound { .. })) =
                                        self.wallet_sdk.accounts_api().get_default()
                                    {
                                        if let Err(error) =
                                            self.wallet_sdk.accounts_api().set_default_account(substate_id)
                                        {
                                            error!(target: LOG_TARGET, "Failed to set default account: {:?}", error);
                                        }
                                    }

                                    // add substate
                                    if let Err(error) = self.wallet_sdk.substate_api().save_root(
                                        result.created_by_transaction,
                                        VersionedSubstateId::new(result.address, result.version),
                                        vec![],
                                    ) {
                                        error!(target: LOG_TARGET, "Failed to save substate: {:?}", error);
                                    }

                                    return true;
                                }
                            },
                            Err(error) => {
                                error!(target: LOG_TARGET, "Error getting public key: {:?}", error);
                            },
                        }
                    }
                }
            },
            Err(error) => {
                error!(target: LOG_TARGET, "Error getting account substate: {}", error);
            },
        }
        false
    }

    pub async fn scan(&self) {
        info!(target: LOG_TARGET, "Scanning accounts...");

        // wait for indexer to be ready
        loop {
            match self.wallet_sdk.get_network_interface().indexer_ready().await {
                Ok(ready) => {
                    if ready {
                        break;
                    } else {
                        warn!(target: LOG_TARGET, "Indexer is not ready yet...");
                    }
                },
                Err(error) => {
                    warn!(target: LOG_TARGET, "Indexer is not ready yet: {error:?}");
                },
            }
            if self.shutdown_signal.is_triggered() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        if self.shutdown_signal.is_triggered() {
            return;
        }

        // if we don't find 10 accounts in a row just stop scanning
        let key_manager_api = self.wallet_sdk.key_manager_api();
        let mut not_found_accounts_count = 0;
        let mut found_accounts_count = 0;
        for i in 0..=u64::MAX {
            if let Ok(public_key) = key_manager_api.get_public_key(TRANSACTION_BRANCH, Some(i)) {
                let component_address = new_component_address_from_public_key(&ACCOUNT_TEMPLATE_ADDRESS, &public_key);
                if self.check_account(&SubstateId::Component(component_address)).await {
                    not_found_accounts_count = 0;
                    found_accounts_count += 1;
                } else {
                    not_found_accounts_count += 1;
                }
                if not_found_accounts_count == 10 {
                    break;
                }
            }
        }

        info!(target: LOG_TARGET, "Scanning accounts finished! {found_accounts_count} owned account(s) found!");
    }
}
