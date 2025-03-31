// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use log::{error, info};
use tari_common_types::types::PublicKey;
use tari_crypto::{ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_dan_common_types::{optional::IsNotFoundError, substate_type::SubstateType, Epoch, VersionedSubstateId};
use tari_dan_wallet_sdk::{
    apis::{
        accounts::AccountsApiError,
        key_manager::{KeyManagerApiError, TRANSACTION_BRANCH},
        transaction::TransactionApiError,
    },
    models::Account,
    network::{SubstateQueryResult, TransactionFinalizedResult, WalletNetworkInterface},
    storage::{WalletStorageError, WalletStore},
    DanWalletSdk,
};
use tari_engine_types::{published_template::PublishedTemplateAddress, substate::SubstateId};
use tari_key_manager::key_manager::DerivedKey;
use tari_shutdown::ShutdownSignal;
use tari_template_abi::TemplateDef;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::models::{ComponentAddress, TemplateAddress};
use tari_transaction::{TransactionId, TransactionV1};

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

    // TODO: somehow get timestamps of transactions right (or check if its possible)
    // TODO: check if we will have transactions saved when other wallet daemons are transactioning
    async fn handle_found_tx_receipt(&self, substate_id: &SubstateId) {
        let network_interface = self.wallet_sdk.get_network_interface();
        match network_interface.query_substate(substate_id, None, false).await {
            Ok(result) => {
                if let Some(receipt) = result.substate.as_transaction_receipt() {
                    let transaction_id = TransactionId::new(receipt.transaction_hash.into_array());
                    if let Ok(tx_resp) = network_interface.get_transaction(transaction_id).await {
                        match self
                            .wallet_sdk
                            .transaction_api()
                            .insert_new_transaction(
                                tx_resp.transaction,
                                vec![],
                                None,
                                false,
                                Some(tx_resp.created_at_timestamp),
                                tx_resp.finalized_at_timestamp,
                            )
                            .await
                        {
                            Ok(_) => {
                                if let Err(error) = self
                                    .wallet_sdk
                                    .transaction_api()
                                    .check_and_store_finalized_transaction(transaction_id)
                                    .await
                                {
                                    error!(target: LOG_TARGET, "Failed to store transaction: {}", error);
                                }
                            },
                            Err(error) => {
                                error!(target: LOG_TARGET, "Failed to store transaction: {}", error);
                            },
                        }
                    }
                }
            },
            Err(error) => {
                error!(target: LOG_TARGET, "Error getting transaction receipt substate: {}", error);
            },
        }
    }

    async fn handle_found_template(&self, substate_id: &SubstateId) {
        let network_interface = self.wallet_sdk.get_network_interface();
        match network_interface.query_substate(substate_id, None, false).await {
            Ok(result) => {
                if let Some(template) = result.substate.as_template() {
                    let key_manager = self.wallet_sdk.key_manager_api();

                    // save template
                    if let Some(template_address) = result.address.as_template() {
                        if let Ok((key_index, _)) =
                            key_manager.get_key_for_public_key(TRANSACTION_BRANCH, &template.author)
                        {
                            let mut template_definition = None::<TemplateDef>;
                            loop {
                                if self.shutdown_signal.is_triggered() {
                                    break;
                                }
                                match network_interface
                                    .fetch_template_definition(template_address.as_hash())
                                    .await
                                {
                                    Ok(template_def) => {
                                        template_definition = Some(template_def);
                                        break;
                                    },
                                    Err(error) => {
                                        error!(target: LOG_TARGET, "Failed to fetch template definition: {}", error);
                                        tokio::time::sleep(Duration::from_secs(1)).await;
                                    },
                                }
                            }
                            if self.shutdown_signal.is_triggered() {
                                return;
                            }
                            if let Some(template_definition) = template_definition {
                                if let Err(error) = self
                                    .wallet_sdk
                                    .template_api()
                                    .add_authored_template(key_index, template_address.as_hash(), template_definition)
                                    .await
                                {
                                    error!(target: LOG_TARGET, "Failed to add authored template: {}", error);
                                }
                            }
                        }
                    }
                }
            },
            Err(error) => {
                error!(target: LOG_TARGET, "Error getting template substate: {}", error);
            },
        }
    }

    /// Handling a found account in substates to see if it is an owned one.
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
                },
                Err(error) => {
                    error!(target: LOG_TARGET, "Error scanning events: {}", error);
                },
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        if self.shutdown_signal.is_triggered() {
            return;
        }

        // Note: we can't be sure which resource related to an account should be saved, so thatswhy
        // the 2 round of iterations here (first get all accounts, then all related resources)

        info!(target: LOG_TARGET, "Scanning accounts...");

        // scanning through all substates to collect all accounts first
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
                        if let SubstateId::Component(_) = substate.substate_id {
                            if let Some(template_address) = substate.template_address {
                                if template_address == ACCOUNT_TEMPLATE_ADDRESS {
                                    self.handle_found_account(&substate.substate_id).await;
                                }
                            }
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

        info!(target: LOG_TARGET, "Scanning accounts resources...");

        // scanning through again through all substates to collect all owned account related resources
        components_offset = 0;
        loop {
            match network_interface
                .list_substates(None, None, Some(10), Some(components_offset))
                .await
            {
                Ok(substates) => {
                    if substates.substates.is_empty() {
                        break;
                    }
                    for substate in substates.substates {
                        match substate.substate_id {
                            SubstateId::Component(_) => {},
                            SubstateId::Resource(_) => {},
                            SubstateId::Vault(_) => {},
                            SubstateId::UnclaimedConfidentialOutput(_) => {},
                            SubstateId::NonFungible(_) => {},
                            SubstateId::NonFungibleIndex(_) => {},
                            SubstateId::TransactionReceipt(_) => {
                                self.handle_found_tx_receipt(&substate.substate_id).await;
                            },
                            SubstateId::Template(_) => {
                                self.handle_found_template(&substate.substate_id).await;
                            },
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

        info!(target: LOG_TARGET, "Scanning finished!");
    }
}
