//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    array,
    collections::{HashMap, HashSet},
};

use log::*;
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    Network,
    NumPreshards,
    StateVersion,
    UtxoSpent,
    UtxoUnspent,
    UtxoUpdate,
};
use tari_template_lib::models::ResourceAddress;

use crate::{
    apis::{
        config::{ConfigApi, ConfigApiError},
        key_manager::{KeyManagerApi, KeyManagerApiError},
        stealth_crypto::StealthCryptoApi,
        stealth_outputs::{StealthOutputsApi, StealthOutputsApiError},
    },
    models::AccountWithPublicKey,
    network::WalletNetworkInterface,
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
};

const LOG_TARGET: &str = "tari::ootle::wallet::apis::stealth_outputs";

// TODO: either fetch num preshards from the network or we should hardcode it to a single value for all apps
const NUM_PRESHARDS: NumPreshards = NumPreshards::P256;

pub struct StealthScannerApi<'a, TStore, TNetworkInterface> {
    store: &'a TStore,
    crypto_api: StealthCryptoApi,
    key_manager_api: KeyManagerApi<'a, TStore>,
    outputs_api: StealthOutputsApi<'a, TStore>,
    config_api: ConfigApi<'a, TStore>,
    network_interface: &'a TNetworkInterface,
}

impl<'a, TStore: WalletStore, TNetworkInterface: WalletNetworkInterface>
    StealthScannerApi<'a, TStore, TNetworkInterface>
{
    pub fn new(
        store: &'a TStore,
        stealth_crypto_api: StealthCryptoApi,
        key_manager_api: KeyManagerApi<'a, TStore>,
        outputs_api: StealthOutputsApi<'a, TStore>,
        network_interface: &'a TNetworkInterface,
        config_api: ConfigApi<'a, TStore>,
    ) -> Self {
        Self {
            store,
            crypto_api: stealth_crypto_api,
            key_manager_api,
            outputs_api,
            network_interface,
            config_api,
        }
    }

    pub async fn scan_and_recover_utxos(
        &self,
        account: &AccountWithPublicKey,
        resource_address: &ResourceAddress,
    ) -> Result<(), StealthScannerApiError> {
        let network = self.config_api.get_network()?;
        let tag = self
            .crypto_api
            .derive_stealth_output_tag(network, account.owner_public_key());

        let mut shard_state_versions_to_set = HashMap::new();
        loop {
            let mut shard_state_versions = self
                .store
                .with_read_tx(|tx| tx.shard_state_version_get(account.address(), resource_address))?;

            // Populate any missing shards with zero
            for shard in NUM_PRESHARDS.all_shards_iter() {
                shard_state_versions.entry(shard).or_insert_with(StateVersion::zero);
            }

            info!(
                target: LOG_TARGET,
                "🔍️ Scanning for stealth outputs (account {}, resource {}, num shards {}, tag {})",
                account.address(),
                resource_address,
                shard_state_versions.len(),
                tag
            );

            let response = self
                .network_interface
                .query_stealth_utxo_updates(*resource_address, shard_state_versions, HashSet::from([tag]))
                .await
                .map_err(|e| StealthScannerApiError::NetworkInterfaceError(e.into()))?;

            // Keep going until there are no more
            if response.updates.is_empty() {
                info!(
                    target: LOG_TARGET,
                    "🔍️ Scan complete for account {}: No more stealth outputs found",
                    account.address()
                );
                // Update state versions to avoid rescanning from previous versions that didnt contain any changes
                self.store.with_write_tx(|tx| {
                    tx.shard_state_version_set_many(
                        account.address(),
                        resource_address,
                        response.per_shard_high_watermark,
                    )
                })?;

                break;
            }

            let num_received = response.updates.len();
            let mut num_recovered = 0;
            let mut num_spent = 0;

            for update in response.updates {
                match update {
                    UtxoUpdate::Unspent(unspent) => {
                        shard_state_versions_to_set
                            .entry(unspent.shard)
                            .and_modify(|v| {
                                if unspent.state_version > *v {
                                    *v = unspent.state_version
                                }
                            })
                            .or_insert(unspent.state_version);

                        if self.check_unspent_utxo_and_store(network, account.key_index(), resource_address, unspent)? {
                            num_recovered += 1;
                        }
                    },
                    UtxoUpdate::Spent(spent) => {
                        shard_state_versions_to_set
                            .entry(spent.shard)
                            .and_modify(|v| {
                                if spent.state_version > *v {
                                    *v = spent.state_version
                                }
                            })
                            .or_insert(spent.state_version);
                        if self.spend(spent)? {
                            num_spent += 1;
                        }
                    },
                }
            }

            self.store.with_write_tx(|tx| {
                tx.shard_state_version_set_many(
                    account.address(),
                    resource_address,
                    shard_state_versions_to_set.drain(),
                )
            })?;

            info!(
                target: LOG_TARGET,
                "Scan complete for account {}: Discovered {}/{} new stealth outputs, marked {} as spent",
                account.address(),
                num_recovered,
                num_received,
                num_spent
            );
        }

        Ok(())
    }

    fn check_unspent_utxo_and_store(
        &self,
        network: Network,
        account_key_index: u64,
        resource_address: &ResourceAddress,
        unspent: UtxoUnspent,
    ) -> Result<bool, StealthScannerApiError> {
        let address = unspent.address;
        let commitment = address.id().into_commitment_bytes();
        let account_key = self.key_manager_api.derive_account_key_pair(account_key_index)?;

        let Some(output) = self.outputs_api.validate_utxo(
            array::from_ref(&account_key),
            network,
            *resource_address,
            commitment,
            &unspent.utxo,
        )?
        else {
            // The output could be burnt, frozen, or otherwise invalid. If we already have it in the db, update its
            // status
            self.outputs_api
                .update_utxo_status_from_utxo(&address, &unspent.utxo)
                .optional()?;

            return Ok(false);
        };

        self.outputs_api.upsert_utxo(&output)?;
        info!(target: LOG_TARGET, "💰️ Recovered stealth output {} for account {}", address, account_key.public_key);
        Ok(true)
    }

    fn spend(&self, spent: UtxoSpent) -> Result<bool, StealthScannerApiError> {
        match self
            .store
            .with_write_tx(|tx| tx.stealth_outputs_mark_as_spent(&spent.address))
            .optional()?
        {
            Some(_) => {
                info!(target: LOG_TARGET, "Marked stealth output {} as spent", spent.address);
                Ok(true)
            },
            None => {
                debug!(
                    target: LOG_TARGET,
                    "Spent UTXO {} not found in wallet store (likely not our output)", spent.address
                );
                Ok(false)
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StealthScannerApiError {
    #[error("Config error: {0}")]
    ConfigError(#[from] ConfigApiError),
    #[error("Network interface error: {0}")]
    NetworkInterfaceError(anyhow::Error),
    #[error("Unexpected response: {details}")]
    UnexpectedResponse { details: String },
    #[error("Store error: {0}")]
    StoreError(#[from] WalletStorageError),
    #[error("Stealth outputs error: {0}")]
    StealthOutputsError(#[from] StealthOutputsApiError),
    #[error("Key manager error: {0}")]
    KeyManagerError(#[from] KeyManagerApiError),
}

impl IsNotFoundError for StealthScannerApiError {
    fn is_not_found_error(&self) -> bool {
        match self {
            Self::StoreError(e) => e.is_not_found_error(),
            Self::StealthOutputsError(e) => e.is_not_found_error(),
            _ => false,
        }
    }
}
