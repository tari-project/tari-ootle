// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::error;
use tari_common_types::types::PublicKey;
use tari_crypto::tari_utilities::ByteArray;
use tari_dan_common_types::optional::IsNotFoundError;
use tari_dan_common_types::substate_type::SubstateType;
use tari_dan_wallet_sdk::apis::key_manager::TRANSACTION_BRANCH;
use tari_dan_wallet_sdk::network::WalletNetworkInterface;
use tari_dan_wallet_sdk::storage::WalletStore;
use tari_dan_wallet_sdk::DanWalletSdk;
use tari_engine_types::substate::SubstateId;
use tari_shutdown::ShutdownSignal;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::models::{ComponentAddress, TemplateAddress};

const LOG_TARGET: &str = "wallet::sdk::resource_scanner";

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
    pub fn new(
        wallet_sdk: DanWalletSdk<TStore, TNetworkInterface>,
        shutdown_signal: ShutdownSignal,
    ) -> Self {
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
                        let public_key = PublicKey::from_canonical_bytes(owner_public_key.as_bytes());
                        // TODO: continue impl
                        let key_manager_api = self.wallet_sdk.key_manager_api();
                        key_manager_api.get_key_for_public_key(TRANSACTION_BRANCH, )
                    }
                }
            }
            Err(error) => {
                error!(target: LOG_TARGET, "Error getting account substate: {}", error);
            }
        }
    }

    pub async fn scan(&self) {
        let network_interface = self.wallet_sdk.get_network_interface();

        let mut components_offset = 0;
        loop {
            match network_interface.list_substates(None, Some(SubstateType::Component), Some(100), Some(components_offset)).await {
                Ok(substates) => {
                    for substate in substates.substates {
                        match substate.substate_id {
                            SubstateId::Component(_) => {
                                if let Some(template_address) = substate.template_address {
                                    if template_address == ACCOUNT_TEMPLATE_ADDRESS {
                                        self.handle_found_account(&substate.substate_id).await;
                                    }
                                }
                            }
                            SubstateId::Resource(_) => {}
                            SubstateId::Vault(_) => {}
                            SubstateId::UnclaimedConfidentialOutput(_) => {}
                            SubstateId::NonFungible(_) => {}
                            SubstateId::NonFungibleIndex(_) => {}
                            SubstateId::TransactionReceipt(_) => {}
                            SubstateId::Template(_) => {}
                            SubstateId::ValidatorFeePool(_) => {}
                        }
                    }
                }
                Err(error) => {
                    error!(target: LOG_TARGET, "Error listing substates: {}", error);
                    break;
                }
            }
            components_offset = components_offset + 100;
        }
    }
}