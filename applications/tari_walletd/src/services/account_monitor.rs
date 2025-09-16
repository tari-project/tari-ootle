//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use log::*;
use tari_engine_types::{
    indexed_value::{IndexedValueError, IndexedWellKnownTypes},
    non_fungible::NonFungibleContainer,
    resource::Resource,
    substate::{Substate, SubstateDiff, SubstateId, SubstateValue},
    vault::Vault,
};
use tari_ootle_common_types::optional::{IsNotFoundError, Optional};
use tari_ootle_wallet_sdk::{
    apis::{
        accounts::AccountsApiError,
        confidential_outputs::ConfidentialOutputsApiError,
        non_fungible_tokens::NonFungibleTokensApiError,
        resources::ResourcesApiError,
        stealth_outputs::StealthOutputsApiError,
        substate::{SubstateApiError, ValidatorScanResult},
        transaction::TransactionApiError,
    },
    models::{AccountUpdate, NewAccountData, NonFungibleToken},
    network::{StatusResponseError, WalletNetworkInterface},
    storage::WalletStore,
    WalletSdk,
};
use tari_shutdown::ShutdownSignal;
use tari_template_builtin::ACCOUNT_TEMPLATE_ADDRESS;
use tari_template_lib::{
    models::{NonFungibleAddress, VaultId},
    prelude::{ComponentAddress, NonFungibleId, ResourceAddress, XTR},
    resource::TOKEN_SYMBOL,
};
use tari_transaction::TransactionId;
use tokio::{
    sync::{mpsc, oneshot},
    time,
    time::MissedTickBehavior,
};

use crate::{
    notify::Notify,
    services::{stealth_utxo_scanner::UtxoScannerHandle, AccountChangedEvent, AccountCreatedEvent, Reply, WalletEvent},
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::account_monitor";

pub struct AccountMonitor<TStore, TNetworkInterface> {
    notify: Notify<WalletEvent>,
    wallet_sdk: WalletSdk<TStore, TNetworkInterface>,
    request_rx: mpsc::Receiver<AccountMonitorRequest>,
    pending_accounts: HashMap<TransactionId, NewAccountData>,
    utxo_scanner_handle: UtxoScannerHandle,
    shutdown_signal: ShutdownSignal,
}

impl<TStore, TNetworkInterface> AccountMonitor<TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError + StatusResponseError,
{
    pub fn new(
        notify: Notify<WalletEvent>,
        wallet_sdk: WalletSdk<TStore, TNetworkInterface>,
        utxo_scanner_handle: UtxoScannerHandle,
        shutdown_signal: ShutdownSignal,
    ) -> (Self, AccountMonitorHandle) {
        let (request_tx, request_rx) = mpsc::channel(1);

        (
            Self {
                notify,
                wallet_sdk,
                request_rx,
                pending_accounts: HashMap::new(),
                utxo_scanner_handle,
                shutdown_signal,
            },
            AccountMonitorHandle { sender: request_tx },
        )
    }

    pub async fn run(mut self) -> Result<(), anyhow::Error> {
        let mut events_subscription = self.notify.subscribe();
        let mut poll_interval = time::interval(Duration::from_secs(60));
        poll_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = self.shutdown_signal.wait() => {
                    break Ok(());
                }

                _ = poll_interval.tick() => {
                    trace!(target: LOG_TARGET, "Polling for transactions");
                    self.on_poll().await;
                }

                Some(req) = self.request_rx.recv() => {
                    self.handle_request(req).await;
                }

                Ok(event) = events_subscription.recv() => {
                    if let Err(e) = self.on_event(event).await {
                        error!(target: LOG_TARGET, "Error handling event: {}", e);
                    }
                },
            }
        }
    }

    async fn handle_request(&self, req: AccountMonitorRequest) {
        match req {
            AccountMonitorRequest::RefreshAccount { account, reply } => {
                let _ignore = reply.send(self.refresh_account(account).await);
            },
        }
    }

    async fn on_poll(&self) {
        if let Err(err) = self.refresh_all_accounts().await {
            error!(target: LOG_TARGET, "Error refreshing all accounts: {}", err);
        }
    }

    async fn refresh_all_accounts(&self) -> Result<(), AccountMonitorError> {
        let accounts_api = self.wallet_sdk.accounts_api();
        // TODO: There could be more than 100 accounts
        let accounts = accounts_api.get_many(0, 100)?;
        for account in accounts {
            info!(
                target: LOG_TARGET,
                "👁️‍🗨️ Refreshing account {}", account
            );
            let is_updated = self.refresh_account(account.component_address).await?;

            if is_updated {
                self.notify.notify(AccountChangedEvent {
                    account_address: account.component_address,
                });
            } else {
                info!(
                    target: LOG_TARGET,
                    "👁️‍🗨️ Account {} is up to date", account
                );
            }
        }
        Ok(())
    }

    async fn refresh_account(&self, account_address: ComponentAddress) -> Result<bool, AccountMonitorError> {
        let substate_api = self.wallet_sdk.substate_api();
        let accounts_api = self.wallet_sdk.accounts_api();

        let Some(account) = accounts_api.get_account_by_address(&account_address).optional()? else {
            // This is not our account
            return Ok(false);
        };

        let mut is_updated = false;
        let maybe_scan_result = substate_api
            .fetch_substate_from_network(&account_address.into(), None)
            .await
            .optional()?;

        let Some(ValidatorScanResult {
            id: account_substate_id,
            substate: account_value,
        }) = maybe_scan_result
        else {
            if account.is_confirmed_on_chain() {
                warn!(target: LOG_TARGET, "❓️ Account {} does not exist according to indexer but confirmed_on_chain = true", account_address);
            }
            // Otherwise, the account is not on-chain, so we wouldn't expect the indexer to have it

            // Scan for associated stealth resources
            self.refresh_stealth_utxos(account_address)?;

            return Ok(false);
        };

        if !account.is_confirmed_on_chain() {
            // Mark the account as on-chain if it is not already
            self.mark_account_as_on_chain(&account_address)?;
            is_updated = true;
        }

        let indexed_value = IndexedWellKnownTypes::from_value(account_value.component().unwrap().state())?;
        substate_api.save_root(account_substate_id.as_ref(), indexed_value.referenced_substates())?;
        let known_child_vaults = substate_api
            .load_dependent_substates(&[account_substate_id.substate_id()])?
            .into_iter()
            .filter(|s| s.substate_id().is_vault())
            .map(|s| {
                let version = s.version();
                (s.into_substate_id(), version)
            })
            .collect::<HashMap<_, _>>();
        for vault_id in indexed_value.vault_ids() {
            let vault_substate_id = SubstateId::Vault(*vault_id);
            let Some(ValidatorScanResult {
                id: versioned_vault_substate_id,
                substate,
                ..
            }) = substate_api
                .fetch_substate_from_network(&vault_substate_id, None)
                .await
                .optional()?
            else {
                warn!(target: LOG_TARGET, "Vault {} for account {} does not exist according to indexer", vault_substate_id, account_substate_id);
                continue;
            };

            let maybe_vault_version = known_child_vaults.get(&vault_substate_id).copied().flatten();
            if let Some(vault_version) = maybe_vault_version {
                // The first time a vault is found, know about the vault substate from the tx result but never added
                // it to the database.
                if versioned_vault_substate_id.version() == vault_version && accounts_api.has_vault(vault_id)? {
                    info!(target: LOG_TARGET, "Vault {} is up to date", versioned_vault_substate_id.substate_id());
                    continue;
                }
            }

            let SubstateValue::Vault(latest_vault) = substate else {
                error!(target: LOG_TARGET, "Substate {} is not a vault. This should be impossible.", vault_substate_id);
                continue;
            };

            // // TODO: this is expensive for many NFTs
            // let mut nfts = HashMap::with_capacity(latest_vault.get_non_fungible_ids().len());
            // if !latest_vault.get_non_fungible_ids().is_empty() {
            //     info!(
            //         target: LOG_TARGET,
            //         "Found {} non-fungible(s) in vault {}. Collecting NFT data for update",
            //         latest_vault.get_non_fungible_ids().len(),
            //         vault_id
            //     );
            // }
            // for nft in latest_vault.get_non_fungible_ids() {
            //     let addr = NonFungibleAddress::new(*latest_vault.resource_address(), nft.clone());
            //     let Some(ValidatorScanResult { address, substate }) =
            //         substate_api.scan_for_substate(&addr.into(), None).await.optional()?
            //     else {
            //         warn!(target: LOG_TARGET, "Non-fungible {} for vault {} does not exist according to indexer",
            // nft, vault_id);         continue;
            //     };
            //     substate_api.save_child(versioned_vault_substate_id.substate_id(), address.as_ref(), [
            //         (*latest_vault.resource_address()).into(),
            //     ])?;
            //     let nft_container = substate.into_non_fungible().ok_or_else(|| {
            //         AccountMonitorError::UnexpectedSubstate(format!("Expected {} to be a non-fungible token.", nft))
            //     })?;
            //     info!(
            //         target: LOG_TARGET,
            //         "Found non-fungible {} in vault {}",
            //         nft,
            //         vault_id
            //     );
            //     nfts.insert(nft.clone(), nft_container);
            // }

            is_updated = true;

            // Save the vault substate
            substate_api.save_child(
                account_substate_id.substate_id(),
                versioned_vault_substate_id.as_ref(),
                [SubstateId::from(*latest_vault.resource_address())],
            )?;

            if let Some(account_addr) = account_substate_id.substate_id().as_component_address() {
                self.add_vault_to_account_if_not_exist(&account_addr, *vault_id, &latest_vault)
                    .await?;
                self.refresh_vault(account_addr, *vault_id, &latest_vault, Default::default())
                    .await?;
            }
        }

        // Scan for all stealth resources
        self.refresh_stealth_utxos(account_address)?;

        Ok(is_updated)
    }

    fn refresh_stealth_utxos(&self, account_address: ComponentAddress) -> Result<(), AccountMonitorError> {
        let mut stealth_resources = self
            .wallet_sdk
            .accounts_api()
            .get_associated_stealth_resources(&account_address)?;
        stealth_resources.insert(XTR);

        info!(
            target: LOG_TARGET,
            "👁️‍🗨️ Requesting UTXO scan for account {} for {} stealth resource(s)",
            account_address,
            stealth_resources.len()
        );
        for resource_address in stealth_resources {
            self.utxo_scanner_handle.request_scan(account_address, resource_address);
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn refresh_vault(
        &self,
        account_address: ComponentAddress,
        vault_id: VaultId,
        latest_vault: &Vault,
        updated_nft_data: HashMap<NonFungibleId, NonFungibleContainer>,
    ) -> Result<(), AccountMonitorError> {
        let accounts_api = self.wallet_sdk.accounts_api();

        let new_balance = latest_vault.balance();
        if !accounts_api.exists_by_address(&account_address)? {
            // This is not our account
            return Ok(());
        }
        let mut has_changed = false;

        let maybe_current_vault = accounts_api.get_vault(&vault_id).optional()?;
        match maybe_current_vault {
            Some(ref vault) => {
                if vault.account_address != account_address {
                    return Err(AccountMonitorError::UnexpectedSubstate(format!(
                        "Vault {} is not in account {}",
                        vault_id, account_address
                    )));
                }
            },
            None => {
                info!(
                    target: LOG_TARGET,
                    "🔒️ NEW vault {} in account {}",
                    vault_id,
                    account_address
                );

                let resource = self.fetch_resource(*latest_vault.resource_address()).await?;
                let token_symbol = resource.token_symbol().map(|s| s.to_string());
                let divisibility = resource.divisibility();

                accounts_api.add_vault(
                    account_address,
                    vault_id,
                    *latest_vault.resource_address(),
                    latest_vault.resource_type(),
                    token_symbol,
                    divisibility,
                )?;
                has_changed = true;
            },
        }

        info!(
            target: LOG_TARGET,
            "🔒️ vault {} in account {} has new balance {}",
            vault_id,
            account_address,
            new_balance
        );
        if let Some(commitments) = latest_vault.get_confidential_commitments() {
            info!(
                target: LOG_TARGET,
                "🔒️ vault {} in account {} has {} confidential commitments",
                vault_id,
                account_address,
                commitments.len()
            );
            let account = self
                .wallet_sdk
                .accounts_api()
                .get_account_by_address(&account_address)?;
            self.wallet_sdk
                .confidential_outputs_api()
                .verify_and_update_confidential_outputs(account.account(), vault_id, commitments)?;
            has_changed = true;
        }

        if latest_vault.resource_type().is_non_fungible() {
            info!(
                target: LOG_TARGET,
                "🔒️ updating vault {} in account {} which has {} non-fungible(s)",
                vault_id,
                account_address,
                latest_vault.get_non_fungible_ids().len()
            );
            self.update_vault_nfts(vault_id, latest_vault, updated_nft_data).await?;
        }

        // If the vault has revealed stealth tokens, we should also to scan for UTXOs
        if latest_vault.resource_type().is_stealth() {
            self.wallet_sdk
                .accounts_api()
                .associate_stealth_resource(&account_address, *latest_vault.resource_address())?;
        }

        let outputs_api = self.wallet_sdk.confidential_outputs_api();
        let new_confidential_balance = outputs_api.get_unspent_balance(&vault_id)?;

        if maybe_current_vault
            .is_none_or(|v| v.confidential_balance != new_confidential_balance || v.revealed_balance != new_balance)
        {
            info!(
                target: LOG_TARGET,
                "🔒️ balance updated in vault {} in account {}. Balance is {} and confidential balance is {}",
                vault_id,
                account_address,
                new_balance,
                new_confidential_balance
            );
            accounts_api.update_vault_balance(vault_id, new_balance, new_confidential_balance)?;
            has_changed = true;
        }
        if has_changed {
            self.notify.notify(AccountChangedEvent { account_address });
        }

        Ok(())
    }

    async fn update_vault_nfts(
        &self,
        vault_id: VaultId,
        latest_vault: &Vault,
        mut updated_nft_data_cache: HashMap<NonFungibleId, NonFungibleContainer>,
    ) -> Result<bool, AccountMonitorError> {
        let non_fungibles_api = self.wallet_sdk.non_fungible_api();
        let mut has_changed = false;

        // Existing NFTs
        // TODO: 1_000_000 is arbitrary
        let existing_nft_ids = non_fungibles_api.get_nft_ids_by_vault_id(&vault_id, 1_000_000, 0)?;
        let latest_nft_ids = latest_vault
            .get_non_fungible_ids()
            .iter()
            .cloned()
            .collect::<HashSet<_>>();

        let new_nft_ids = latest_nft_ids
            .difference(&existing_nft_ids)
            .cloned()
            .collect::<Vec<_>>();
        let removed_nft_ids = existing_nft_ids
            .difference(&latest_nft_ids)
            .cloned()
            .collect::<Vec<_>>();

        debug!(
            target: LOG_TARGET,
            "Local vault {} has {} NFTs, and {} NFTs in updated vault, {} new nft(s).",
            vault_id,
            existing_nft_ids.len(),
            latest_vault.get_non_fungible_ids().len(),
            new_nft_ids.len()
        );

        for to_remove in removed_nft_ids {
            info!(
                target: LOG_TARGET,
                "🧸 Updating NFTs: Non-fungible {} has been transferred from vault {}. Removing it from the stored vault.",
                to_remove,
                vault_id
            );
            if non_fungibles_api
                .remove_nft(&vault_id, &to_remove)
                .optional()?
                .is_none()
            {
                warn!(
                    target: LOG_TARGET,
                    "NonFungible ID {} is found by indexer, but not found in the vault.", to_remove
                );
            }
            has_changed = true;
        }

        for id in new_nft_ids {
            info!(
                target: LOG_TARGET,
                "🧸 Updating NFTs: Non-fungible {} has been added to vault {}. Adding it to the stored vault.",
                id,
                vault_id
            );
            let nft = match updated_nft_data_cache.remove(&id) {
                Some(nft) => nft,
                None => {
                    let substate = self
                        .fetch_substate(&NonFungibleAddress::new(*latest_vault.resource_address(), id.clone()).into())
                        .await
                        .optional()?;
                    let Some(substate) = substate else {
                        warn!(target: LOG_TARGET, "Non-fungible {} for vault {} does not exist according to indexer. Unable to update it", id, vault_id);
                        continue;
                    };
                    substate.into_substate_value().into_non_fungible().ok_or_else(|| {
                        AccountMonitorError::UnexpectedSubstate(format!("Expected {} to be a non-fungible token.", id))
                    })?
                },
            };

            let maybe_nft_contents = nft.contents();

            let non_fungible = NonFungibleToken {
                is_burnt: nft.is_burnt(),
                resource_address: *latest_vault.resource_address(),
                vault_id,
                nft_id: id.clone(),
                data: maybe_nft_contents
                    .map(|nft| nft.data().clone())
                    .unwrap_or(tari_bor::Value::Null),
                mutable_data: maybe_nft_contents
                    .map(|nft| nft.mutable_data().clone())
                    .unwrap_or(tari_bor::Value::Null),
            };

            non_fungibles_api.save_nft(&non_fungible)?;
            has_changed = true;
        }

        Ok(has_changed)
    }

    #[allow(clippy::too_many_lines)]
    async fn process_result(&mut self, tx_id: TransactionId, diff: &SubstateDiff) -> Result<(), AccountMonitorError> {
        let substate_api = self.wallet_sdk.substate_api();
        let accounts_api = self.wallet_sdk.accounts_api();

        let mut new_account = None;
        if let Some(account) = self.pending_accounts.remove(&tx_id) {
            let existing_account = accounts_api.get_account_by_address(&account.address).optional()?;
            // Check that the new account was created in this transaction
            if diff.up_iter().any(|(id, _)| *id == account.address) {
                // NOTE: that account must exist by this point
                self.mark_account_as_on_chain(&account.address)?;
                new_account = existing_account;
                debug!(
                    target: LOG_TARGET,
                    "👁️‍🗨️ New account {} created in transaction {}",
                    account.address,
                    tx_id
                );
            } else if existing_account.is_none_or(|acc| !acc.is_confirmed_on_chain()) {
                warn!(
                    target: LOG_TARGET,
                    "⚠️ Transaction {} does not contain the new account {} but the account does not exist or is not on-chain. This should be impossible.",
                    tx_id,
                    account.address,
                );
                // Continue anyway
            } else {
                info!(
                    target: LOG_TARGET,
                    "👁️‍🗨️ Account {} already exists and on-chain (processing transaction result {})",
                    account.address,
                    tx_id
                );
            }
        }

        let mut vaults = diff
            .up_iter()
            .filter(|(a, _)| a.is_vault())
            .map(|(a, s)| (a.as_vault_id().unwrap(), s))
            .collect::<HashMap<_, _>>();

        let accounts =
            diff.up_iter()
                .filter(|(_, s)| is_account(s))
                .filter_map(|(a, s)| {
                    match IndexedWellKnownTypes::from_value(s.substate_value().component().unwrap().state()) {
                        Ok(value) => Some((
                            a.as_component_address().expect("BUG: substate id is a component"),
                            value,
                        )),
                        Err(e) => {
                            error!(
                                target: LOG_TARGET,
                                "👁️‍🗨️ Failed to parse account substate {} in tx {}: {}", a, tx_id, e
                            );
                            None
                        },
                    }
                });

        // Find and process all new/existing vaults
        for (account_addr, value) in accounts {
            // If we know about this account, mark it as on-chain (if it isn't already)
            self.mark_account_as_on_chain(&account_addr).optional()?;
            for vault_id in value.vault_ids() {
                // Any vaults we process here do not need to be reprocesed later
                if let Some(vault) = vaults.remove(vault_id).and_then(|s| s.substate_value().vault()) {
                    self.add_vault_to_account_if_not_exist(&account_addr, *vault_id, vault)
                        .await?;

                    let updated_nfts = vault
                        .get_non_fungible_ids()
                        .iter()
                        .filter_map(|nft_id| {
                            diff.up_iter()
                                .find(|(id, _)| id.as_non_fungible_address().map(|a| a.id()) == Some(nft_id))
                                .map(|(_, s)| {
                                    let nft = s
                                        .substate_value()
                                        .non_fungible()
                                        .unwrap_or_else(|| panic!("Expected {} to be a non-fungible token.", nft_id));
                                    (nft_id.clone(), nft.clone())
                                })
                        })
                        .collect();

                    self.refresh_vault(account_addr, *vault_id, vault, updated_nfts).await?;
                }
            }
        }

        let mut updated_accounts = HashSet::new();
        // Process all existing vaults that belong to an account
        for (vault_id, substate) in vaults {
            let vault_addr = SubstateId::Vault(vault_id);
            let SubstateValue::Vault(vault) = substate.substate_value() else {
                error!(target: LOG_TARGET, "👁️‍🗨️ Substate {} is not a vault. This should be impossible.", vault_addr);
                continue;
            };

            // Try and get the account address from the vault
            let maybe_vault_substate = substate_api.get_substate(&vault_addr).optional()?;
            let Some(vault_substate) = maybe_vault_substate else {
                // This should be impossible.
                error!(target: LOG_TARGET, "👁️‍🗨️ Vault {} is not a known substate.", vault_addr);
                continue;
            };

            let Some(account_addr) = vault_substate.parent_address else {
                // Happens if this is someone else's vault
                debug!(target: LOG_TARGET, "👁️‍🗨️ Vault {} has no parent component.", vault_addr);
                continue;
            };
            let account_addr = account_addr.as_component_address().unwrap_or_else(|| {
                panic!(
                    "BUG: Vault {} has a parent address that is not a component.",
                    vault_addr
                )
            });

            // Check if this vault is associated with an account
            if accounts_api.get_account_by_address(&account_addr).optional()?.is_none() {
                info!(
                    target: LOG_TARGET,
                    "👁️‍🗨️ Vault {} not in any known account",
                    vault_addr,
                );
                continue;
            }

            self.add_vault_to_account_if_not_exist(&account_addr, vault_id, vault)
                .await?;

            let updated_nfts = vault
                .get_non_fungible_ids()
                .iter()
                .filter_map(|nft_id| {
                    diff.up_iter()
                        .find(|(id, _)| id.as_non_fungible_address().map(|a| a.id()) == Some(nft_id))
                        .map(|(_, s)| {
                            let nft = s
                                .substate_value()
                                .non_fungible()
                                .unwrap_or_else(|| panic!("Expected {} to be a non-fungible token.", nft_id));
                            (nft_id.clone(), nft.clone())
                        })
                })
                .collect();

            // Update the vault balance / confidential outputs
            self.refresh_vault(account_addr, vault_id, vault, updated_nfts).await?;
            updated_accounts.insert(account_addr);
        }

        // Update UTXOs
        let stealth_outputs_api = self.wallet_sdk.stealth_outputs_api();
        let utxos = diff.up_iter().filter(|(id, _)| id.is_utxo()).map(|(id, s)| {
            let utxo = s
                .substate_value()
                .as_utxo()
                .unwrap_or_else(|| panic!("Expected {} to be a UTXO.", id));
            (id.as_utxo_address().expect("is_utxo checked"), utxo)
        });

        // TODO: if we submitted this transaction, we could let this part of the code know which outputs are ours
        stealth_outputs_api.verify_and_update_outputs(utxos)?;

        if let Some(account) = new_account {
            debug!(
                target: LOG_TARGET,
                "👁️‍🗨️ Notifying account created for tx {}: {}",
                tx_id,
                account
            );
            self.notify.notify(AccountCreatedEvent {
                account: account.account,
                _created_by_tx: tx_id,
            });
        }
        if !updated_accounts.is_empty() {
            debug!(
                target: LOG_TARGET,
                "👁️‍🗨️ Notifying {} account(s) changed for tx {}",
                updated_accounts.len(),
                tx_id
            );
            for account_address in updated_accounts {
                self.notify.notify(AccountChangedEvent { account_address });
            }
        }

        Ok(())
    }

    async fn fetch_resource(&self, resx_addr: ResourceAddress) -> Result<Resource, AccountMonitorError> {
        if let Some(resx) = self.wallet_sdk.resources_api().get(&resx_addr).optional()? {
            return Ok(resx);
        }

        let substate = self.fetch_substate(&SubstateId::Resource(resx_addr)).await?;
        let resx = substate.into_substate_value().into_resource().ok_or_else(|| {
            AccountMonitorError::UnexpectedSubstate(format!("Expected {} to be a resource.", resx_addr))
        })?;
        self.wallet_sdk.resources_api().upsert_resource(&resx_addr, &resx)?;
        Ok(resx)
    }

    async fn fetch_substate(&self, substate_id: &SubstateId) -> Result<Substate, AccountMonitorError> {
        let substate_api = self.wallet_sdk.substate_api();
        let ValidatorScanResult { substate, id: address } =
            substate_api.fetch_substate_from_network(substate_id, None).await?;
        Ok(Substate::new(address.version(), substate))
    }

    fn mark_account_as_on_chain(&self, account_addr: &ComponentAddress) -> Result<(), AccountMonitorError> {
        self.wallet_sdk
            .accounts_api()
            .update_account(account_addr, AccountUpdate {
                is_account_on_chain: Some(true),
                ..Default::default()
            })?;
        Ok(())
    }

    async fn add_vault_to_account_if_not_exist(
        &self,
        account_addr: &ComponentAddress,
        vault_id: VaultId,
        vault: &Vault,
    ) -> Result<(), AccountMonitorError> {
        let accounts_api = self.wallet_sdk.accounts_api();
        if !accounts_api.exists_by_address(account_addr)? {
            // This is not our account
            return Ok(());
        }
        if accounts_api.has_vault(&vault_id)? {
            return Ok(());
        }
        let maybe_resource = match self.fetch_resource(*vault.resource_address()).await {
            Ok(r) => Some(r),
            Err(e) => {
                warn!(
                    target: LOG_TARGET,
                    "👁️‍🗨️ Failed to scan vault {} from VN: {}",
                    vault_id,
                    e
                );
                None
            },
        };

        let token_symbol = maybe_resource
            .as_ref()
            .and_then(|r| r.metadata().get(TOKEN_SYMBOL).map(|s| s.to_string()));
        let divisibility = maybe_resource.as_ref().map(|r| r.divisibility()).unwrap_or(0);

        info!(
            target: LOG_TARGET,
            "👁️‍🗨️ New {} in account {}",
            vault_id,
            account_addr
        );
        accounts_api.add_vault(
            *account_addr,
            vault_id,
            *vault.resource_address(),
            vault.resource_type(),
            token_symbol,
            divisibility,
        )?;

        Ok(())
    }

    async fn on_event(&mut self, event: WalletEvent) -> Result<(), AccountMonitorError> {
        match event {
            WalletEvent::TransactionSubmitted(event) => {
                if let Some(account) = event.new_account {
                    self.pending_accounts.insert(event.transaction_id, account);
                }
            },
            WalletEvent::TransactionFinalized(event) => {
                if let Some(diff) = event.finalize.result.any_accept() {
                    self.process_result(event.transaction_id, diff).await?;
                }
            },
            WalletEvent::TransactionInvalid(event) => {
                self.pending_accounts.remove(&event.transaction_id);
            },
            WalletEvent::AccountCreatedOnChain(_) |
            WalletEvent::AccountChangedOnChain(_) |
            WalletEvent::AuthLoginRequest(_) => {},
        }
        Ok(())
    }
}

#[derive(Debug)]
enum AccountMonitorRequest {
    RefreshAccount {
        account: ComponentAddress,
        reply: Reply<Result<bool, AccountMonitorError>>,
    },
}

#[derive(Debug, Clone)]
pub struct AccountMonitorHandle {
    sender: mpsc::Sender<AccountMonitorRequest>,
}

impl AccountMonitorHandle {
    pub async fn refresh_account(&self, account: ComponentAddress) -> Result<bool, AccountMonitorError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send(AccountMonitorRequest::RefreshAccount {
                account,
                reply: reply_tx,
            })
            .await
            .map_err(|_| AccountMonitorError::ServiceShutdown)?;
        reply_rx.await.map_err(|_| AccountMonitorError::ServiceShutdown)?
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AccountMonitorError {
    #[error("Transaction API error: {0}")]
    Transaction(#[from] TransactionApiError),
    #[error("Accounts API error: {0}")]
    Accounts(#[from] AccountsApiError),
    #[error("Substate API error: {0}")]
    Substate(#[from] SubstateApiError),
    #[error("Outputs API error: {0}")]
    ConfidentialOutputs(#[from] ConfidentialOutputsApiError),
    #[error("Stealth Outputs API error: {0}")]
    StealthOutputs(#[from] StealthOutputsApiError),
    #[error("Non Fungibles API error: {0}")]
    NonFungibleTokens(#[from] NonFungibleTokensApiError),
    #[error("Resources API error: {0}")]
    Resources(#[from] ResourcesApiError),
    #[error("Failed to decode binary value: {0}")]
    DecodeValueFailed(#[from] IndexedValueError),
    #[error("Unexpected substate: {0}")]
    UnexpectedSubstate(String),
    #[error("Monitor service is not running")]
    ServiceShutdown,
}

impl IsNotFoundError for AccountMonitorError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::Substate(s) if s.is_not_found_error())
    }
}

fn is_account(s: &Substate) -> bool {
    s.substate_value()
        .component()
        .filter(|c| c.template_address == ACCOUNT_TEMPLATE_ADDRESS)
        .is_some()
}
