// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::{error, info};
use std::time::Duration;
use tari_common_types::types::PublicKey;
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_dan_common_types::{optional::IsNotFoundError, substate_type::SubstateType, Epoch, VersionedSubstateId};
use tari_dan_wallet_sdk::storage::WalletStorageError;
use tari_dan_wallet_sdk::{
    apis::{
        accounts::AccountsApiError,
        key_manager::{KeyManagerApiError, TRANSACTION_BRANCH},
    },
    network::WalletNetworkInterface,
    storage::WalletStore,
    DanWalletSdk,
};
use tari_engine_types::substate::SubstateId;
use tari_key_manager::key_manager::DerivedKey;
use tari_shutdown::ShutdownSignal;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::models::{ComponentAddress, TemplateAddress};

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

    async fn handle_found_account(&self, substate_id: &SubstateId) {
        let network_interface = self.wallet_sdk.get_network_interface();
        match network_interface.query_substate(substate_id, None, false).await {
            Ok(result) => {
                if let Some(component) = result.substate.as_component() {
                    if let Some(owner_public_key) = component.owner_key {
                        match PublicKey::from_canonical_bytes(owner_public_key.as_bytes()) {
                            Ok(public_key) => {
                                let key_manager_api = self.wallet_sdk.key_manager_api();
                                if let Ok((index, key)) = key_manager_api.get_key_for_public_key_with_max_index(
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
                                    if let Err(AccountsApiError::StoreError(WalletStorageError::NotFound { .. })) = self.wallet_sdk.accounts_api().get_default() {
                                        if let Err(error) = self.wallet_sdk.accounts_api().set_default_account(substate_id)
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

                                    // TODO: get transactions by triggering indexer to scan for transactions 
                                    // TODO: for specific accounts
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
    }

    // TODO: consider running periodically the scan (every couple of minutes)
    // TODO: to avoid missing something from indexer if it did not load something yet from VN
    // TODO: if periodic scanning will happen, make sure we do not trigger scanning for events from epoch 0, only once!
    pub async fn scan(&self) {
        let network_interface = self.wallet_sdk.get_network_interface();
        
        // trigger a full network scan on indexer to make sure we have all the resources stored on indexer
        let mut scan_success = false;
        while !scan_success {
            if self.shutdown_signal.is_triggered() {
                break;
            }
            match network_interface.scan_events(Epoch::from(0)).await {
                Ok(success) => {
                    scan_success = success;
                }
                Err(error) => {
                    error!(target: LOG_TARGET, "Error scanning events: {}", error);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        let mut components_offset = 0;
        loop {
            match network_interface
                .list_substates(None, Some(SubstateType::Component), Some(10), Some(components_offset))
                .await
            {
                Ok(substates) => {
                    if substates.substates.is_empty() {
                        break;
                    }
                    for substate in substates.substates {
                        match substate.substate_id {
                            SubstateId::Component(_) => {
                                if let Some(template_address) = substate.template_address {
                                    if template_address == ACCOUNT_TEMPLATE_ADDRESS {
                                        self.handle_found_account(&substate.substate_id).await;
                                    }
                                }
                            },
                            SubstateId::Resource(_) => {},
                            SubstateId::Vault(_) => {},
                            SubstateId::UnclaimedConfidentialOutput(_) => {},
                            SubstateId::NonFungible(_) => {},
                            SubstateId::NonFungibleIndex(_) => {},
                            SubstateId::TransactionReceipt(_) => {},
                            SubstateId::Template(_) => {},
                            SubstateId::ValidatorFeePool(_) => {},
                        }
                    }
                },
                Err(error) => {
                    error!(target: LOG_TARGET, "Error listing substates: {}", error);
                    break;
                },
            }
            components_offset += 10;
        }
    }
}
