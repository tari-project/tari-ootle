//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{array, time::Duration};

use log::*;
use tari_engine_types::UtxoOutput;
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    Network,
};
use tari_ootle_wallet_sdk::{
    network::{StatusResponseError, WalletNetworkInterface},
    storage::{WalletStore, WalletStoreReader, WalletStoreWriter},
    WalletSdk,
};
use tari_template_lib::models::{ResourceAddress, UtxoAddress, UtxoId};
use tokio::sync::watch;

use crate::utxo_scanner::StealthScannerApiError;

const LOG_TARGET: &str = "tari::ootle::wallet_services::utxo_recovery";

pub struct UtxoRecovery<TStore, TNetworkInterface> {
    sdk: WalletSdk<TStore, TNetworkInterface>,
}

impl<TStore, TNetworkInterface> UtxoRecovery<TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError + StatusResponseError,
{
    pub fn new(sdk: WalletSdk<TStore, TNetworkInterface>) -> Self {
        Self { sdk }
    }

    pub async fn run(mut self, mut waker: watch::Receiver<()>) -> anyhow::Result<()> {
        info!(target: LOG_TARGET, "🚀 Starting UTXO recovery process");
        loop {
            // Process the UTXO validation queue until empty. NOTE: this comes before waiting on the waker so that we
            // check the queue on startup
            if let Err(err) = self.process_utxo_validation_queue().await {
                match &err {
                    // Perhaps intermittent network issue, just log and continue
                    StealthScannerApiError::NetworkInterfaceError(_) |
                    StealthScannerApiError::UnexpectedResponse { .. } => {
                        warn!(target: LOG_TARGET, "⚠️ UTXO recovery process encountered a recoverable error: {}", err);
                        // Sleep to prevent fast spinning
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        // Continue processing the queue
                        continue;
                    },
                    _ => {
                        // Non-recoverable error
                        error!(target: LOG_TARGET, "❌ UTXO recovery process encountered a non-recoverable error: {}", err);
                        // CRASH
                        return Err(err.into());
                    },
                }
            }

            if waker.changed().await.is_err() {
                info!(target: LOG_TARGET, "🔕 UTXO recovery process terminating because the notification channel closed");
                break;
            }
        }
        Ok(())
    }

    async fn process_utxo_validation_queue(&mut self) -> Result<(), StealthScannerApiError> {
        loop {
            let batch = self
                .sdk
                .store()
                .with_read_tx(|tx| tx.utxo_process_queue_fetch_batch(100))?;

            if batch.is_empty() {
                debug!(target: LOG_TARGET, "✅ No more UTXOs to process");
                return Ok(());
            }

            for (resource_addr, tag_and_nonce_to_view_key_map) in &batch {
                if tag_and_nonce_to_view_key_map.is_empty() {
                    error!(target: LOG_TARGET, "❓️ NEVER HAPPEN: Asked indexer for zero UTXOs for resource {}.", resource_addr);
                }
                let tag_and_nonce_pairs = tag_and_nonce_to_view_key_map.keys().copied().collect();

                // max 3.3kB per request (excl underlying protocol overhead *cough* json + hex)
                let utxos = self
                    .sdk
                    .get_network_interface()
                    .get_unspent_utxos(*resource_addr, tag_and_nonce_pairs)
                    .await
                    .map_err(|e| StealthScannerApiError::NetworkInterfaceError(e.into()))?;

                if utxos.is_empty() {
                    // We asked for some UTXOs but got none back. This should never happen because UTXO recovery is
                    // 'fed' by UTXO scanning, which should only give recovery tasks if the network
                    // has UTXOs. This could indicate a bug in the indexer (assuming that NetworkInterface impl is
                    // used). To prevent this case causing fast spinning, return an error that will
                    // sleep and retry.
                    return Err(StealthScannerApiError::UnexpectedResponse {
                        details: format!(
                            "{} UTXOs requested but network returned an empty set for resource {}.",
                            tag_and_nonce_to_view_key_map.len(),
                            resource_addr
                        ),
                    });
                }

                if utxos.len() != tag_and_nonce_to_view_key_map.len() {
                    // We could error as above, but let's process what we got
                    warn!(target: LOG_TARGET, "⚠️ Mismatch in number of UTXOs queried ({}) vs returned by indexer ({}).",
                        tag_and_nonce_to_view_key_map.len(),
                        utxos.len(),
                    );
                }

                // Group by account key index
                let utxos = utxos
                    .into_iter()
                    .filter_map(|(id, utxo)| {
                        let is_frozen = utxo.is_frozen;
                        let Some(output) = utxo.into_output() else {
                            warn!(target: LOG_TARGET, "❓️ NEVER HAPPEN: Indexer returned burnt UTXO {}. Ignoring", id);
                            return None;
                        };
                        Some((id, output, is_frozen))
                    })
                    .filter_map(|(id, output, is_frozen)| {
                        let tag_and_nonce_pair = (output.tag, output.output.public_nonce);
                        let Some(view_key_index) = tag_and_nonce_to_view_key_map.get(&tag_and_nonce_pair).copied() else {
                            warn!(target: LOG_TARGET, "❓️ NEVER HAPPEN: Indexer returned UTXO with tag {}, nonce {} that we didn't request. Ignoring", output.tag, output.output.public_nonce);
                            return None;
                        };

                        Some(FoundUtxo {
                            view_key_index,
                            id,
                            output,
                            is_frozen,
                        })
                    })
                    .collect();

                self.process_recovered_utxos(*resource_addr, utxos)?;
            }
        }
    }

    fn process_recovered_utxos(
        &self,
        resource_address: ResourceAddress,
        utxos_to_recover: Vec<FoundUtxo>,
    ) -> Result<(), StealthScannerApiError> {
        let num_attempted = utxos_to_recover.len();
        let mut num_recovered = 0;
        for utxo in utxos_to_recover {
            if self.check_unspent_utxo_and_store(self.sdk.network(), resource_address, utxo)? {
                num_recovered += 1;
            }
        }
        if num_attempted > 0 {
            info!(
                target: LOG_TARGET,
                "✅ Recovered {} out of {} applicable UTXOs",
                num_recovered,
                num_attempted,
            );
        }

        Ok(())
    }

    fn check_unspent_utxo_and_store(
        &self,
        network: Network,
        resource_address: ResourceAddress,
        found: FoundUtxo,
    ) -> Result<bool, StealthScannerApiError> {
        let view_only_key = self
            .sdk
            .key_manager_api()
            .derive_view_only_keypair(found.view_key_index)?;
        let outputs_api = self.sdk.stealth_outputs_api();

        let address = UtxoAddress::new(resource_address, found.id);
        let commitment = found.id.into_commitment_bytes();
        let Some(output) = outputs_api.validate_utxo(
            array::from_ref(&view_only_key),
            network,
            resource_address,
            commitment,
            &found.output,
            found.is_frozen,
        )?
        else {
            // The output could be burnt, frozen, or otherwise invalid. If we already have it in the db, update its
            // status, if not, do nothing.
            outputs_api
                .update_utxo_status(&address, None, None, Some(found.is_frozen))
                .optional()?;
            self.sdk.store().with_write_tx(|tx| {
                tx.utxo_process_queue_remove_item(resource_address, found.output.tag, found.output.output.public_nonce)
            })?;

            return Ok(false);
        };

        outputs_api.upsert_utxo(&output)?;
        self.sdk.store().with_write_tx(|tx| {
            tx.utxo_process_queue_remove_item(resource_address, found.output.tag, found.output.output.public_nonce)
        })?;
        info!(target: LOG_TARGET, "💰️ Recovered stealth output {} for account {}", address, view_only_key.public_key);
        Ok(true)
    }
}

struct FoundUtxo {
    view_key_index: u64,
    output: UtxoOutput,
    id: UtxoId,
    is_frozen: bool,
}
