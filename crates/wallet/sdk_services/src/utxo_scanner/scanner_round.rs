//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::collections::HashMap;

use log::{debug, info, trace, warn};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_engine_types::ConvertFromByteType;
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    shard::Shard,
    Network,
    NumPreshards,
    StateVersion,
};
use tari_ootle_wallet_sdk::{
    models::{AccountWithAddress, Key, UtxoSpent, UtxoUnspent, WalletUtxoUpdate},
    network::{StatusResponseError, WalletNetworkInterface},
    storage::{WalletStorageError, WalletStore, WalletStoreReader, WalletStoreWriter},
    WalletSdk,
};
use tari_template_lib::models::{ComponentAddress, ResourceAddress};
use tokio::sync::watch;

use crate::utxo_scanner::StealthScannerApiError;

const LOG_TARGET: &str = "tari::ootle::wallet_services::scanner_round";
// TODO: either fetch num preshards from the network or we should hardcode it to a single value for all apps
const NUM_PRESHARDS: NumPreshards = NumPreshards::P256;

pub struct UtxoScannerRound<'a, TStore, TNetworkInterface> {
    network: Network,
    account: &'a AccountWithAddress,
    view_key: &'a Key,
    resource_address: &'a ResourceAddress,

    sdk: &'a WalletSdk<TStore, TNetworkInterface>,
    notify_tx: &'a watch::Sender<()>,

    shard_state_versions_to_set: HashMap<Shard, StateVersion>,
    utxos_to_recover: Vec<(ComponentAddress, UtxoUnspent)>,
    utxos_to_spend: Vec<UtxoSpent>,
}

impl<'a, TStore, TNetworkInterface> UtxoScannerRound<'a, TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError + StatusResponseError,
{
    pub fn new(
        network: Network,
        sdk: &'a WalletSdk<TStore, TNetworkInterface>,
        notify_tx: &'a watch::Sender<()>,
        account: &'a AccountWithAddress,
        view_key: &'a Key,
        resource_address: &'a ResourceAddress,
    ) -> Self {
        Self {
            network,
            sdk,
            notify_tx,
            account,
            view_key,
            resource_address,
            shard_state_versions_to_set: HashMap::new(),
            utxos_to_recover: Vec::new(),
            utxos_to_spend: Vec::new(),
        }
    }

    pub async fn scan_for_utxo_updates(&mut self) -> Result<usize, StealthScannerApiError> {
        let mut num_found = 0;
        loop {
            if !self.scan().await? {
                break;
            }
            num_found += 1;
        }

        if num_found > 0 {
            // Notify that there are new UTXOs to process
            debug!(target: LOG_TARGET, "Notifying that new UTXOs are available for processing");
            let _ignore = self.notify_tx.send(());
        }

        Ok(num_found)
    }

    async fn scan(&mut self) -> Result<bool, StealthScannerApiError> {
        let mut shard_state_versions = self
            .sdk
            .store()
            .with_read_tx(|tx| tx.shard_state_version_get(self.account.component_address(), self.resource_address))?;

        // Populate any missing shards with zero
        for shard in NUM_PRESHARDS.all_shards_iter() {
            shard_state_versions.entry(shard).or_insert_with(StateVersion::zero);
        }

        info!(
            target: LOG_TARGET,
            "🔍️ Scanning for stealth outputs (account {}, resource {}, num shards {})",
            self.account.component_address(),
            self.resource_address,
            shard_state_versions.len(),
        );

        let response = self
            .sdk
            .get_network_interface()
            .query_stealth_utxo_updates(*self.resource_address, shard_state_versions)
            .await
            .map_err(|e| StealthScannerApiError::NetworkInterfaceError(e.into()))?;

        // Keep going until there are no more
        if response.shard_updates.is_empty() {
            info!(
                target: LOG_TARGET,
                "🔍️ Scan complete for account {}: No more stealth outputs found",
                self.account.component_address()
            );
            // Update state versions to avoid rescanning from previous versions that didnt contain any changes
            self.sdk.store().with_write_tx(|tx| {
                tx.shard_state_version_set_many(
                    self.account.component_address(),
                    self.resource_address,
                    response.per_shard_high_watermark,
                )
            })?;

            return Ok(false);
        }

        let num_received = response.shard_updates.len();
        let mut num_spent = 0;

        for (shard, update_set) in response.shard_updates {
            self.shard_state_versions_to_set
                .entry(shard)
                .and_modify(|v| {
                    if update_set.max_state_version > *v {
                        *v = update_set.max_state_version
                    }
                })
                .or_insert(update_set.max_state_version);
            for update in update_set.updates {
                match update {
                    WalletUtxoUpdate::Unspent(unspent) => {
                        if self.check_if_tag_matches(&unspent)? {
                            debug!(
                                target: LOG_TARGET,
                                "🏷️ Stealth output tag {} matches. Queueing for recovery.",
                                unspent.tag
                            );
                            self.utxos_to_recover.push((*self.account.component_address(), unspent));
                        }
                    },
                    WalletUtxoUpdate::Spent(spent) => {
                        self.utxos_to_spend.push(spent);
                    },
                    WalletUtxoUpdate::Burnt(burnt) => {
                        // NOTE: we treat burnt outputs the same as spent for now. In future, we may want to track them
                        // separately
                        self.utxos_to_spend.push(UtxoSpent {
                            id: burnt.id,
                            version: burnt.version,
                        });
                    },
                }
            }
        }

        // Atomically persist all changes from this round
        let num_recovered = self.utxos_to_recover.len();
        self.sdk.store().with_write_tx(|tx| {
            // Mark UTXOs as spent (if they exist)
            for spent in self.utxos_to_spend.drain(..) {
                if Self::spend(tx, self.resource_address, spent)? {
                    num_spent += 1;
                }
            }

            // Queue up tag matching UTXOs for processing
            tx.utxo_process_queue_extend(self.resource_address, self.utxos_to_recover.drain(..))?;

            // Update shard state versions
            tx.shard_state_version_set_many(
                self.account.component_address(),
                self.resource_address,
                self.shard_state_versions_to_set.drain(),
            )
        })?;

        info!(
            target: LOG_TARGET,
            "Scan round complete for account {}: Validated the tag of {}/{} new stealth outputs, marked {} as spent",
            self.account.component_address(),
            num_recovered,
            num_received,
            num_spent
        );

        Ok(true)
    }

    fn check_if_tag_matches(&self, unspent: &UtxoUnspent) -> Result<bool, StealthScannerApiError> {
        let Ok(public_nonce) = RistrettoPublicKey::convert_from_byte_type(&unspent.public_nonce).inspect_err(|e| {
            warn!(
                target: LOG_TARGET,
                "⚠️ Received a malformed public nonce while syncing output: {}. Ignoring output.",
                e
            )
        }) else {
            return Ok(false);
        };

        let tag = self.sdk.stealth_crypto_api().derive_stealth_output_tag(
            self.network,
            &self.view_key.secret,
            &public_nonce,
            self.resource_address,
        );

        if tag != unspent.tag {
            trace!(
                target: LOG_TARGET,
                "Stealth output tag {} does not match (likely not our output)", unspent.tag
            );
            return Ok(false);
        }

        debug!(
            target: LOG_TARGET,
            "Stealth output tag {} matches. Will request full output.", unspent.tag
        );

        Ok(true)
    }

    fn spend(
        tx: &mut TStore::WriteTransaction<'_>,
        resource_address: &ResourceAddress,
        spent: UtxoSpent,
    ) -> Result<bool, WalletStorageError> {
        match tx
            .stealth_outputs_mark_as_spent(resource_address, &spent.id)
            .optional()?
        {
            Some(_) => {
                info!(target: LOG_TARGET, "Marked stealth output {} as spent", spent.id);
                Ok(true)
            },
            None => {
                debug!(
                    target: LOG_TARGET,
                    "Spent UTXO {} not found in wallet store (likely not our output)", spent.id
                );
                Ok(false)
            },
        }
    }
}
