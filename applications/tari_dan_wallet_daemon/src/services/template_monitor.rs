// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{ops::Add, time::Duration};

use anyhow::anyhow;
use log::error;
use tari_common_types::types::PublicKey;
use tari_dan_common_types::optional::IsNotFoundError;
use tari_dan_wallet_sdk::{
    apis::{key_manager, transaction::TransactionApiError},
    models::TransactionStatus,
    network::WalletNetworkInterface,
    storage::WalletStore,
    DanWalletSdk,
};
use tari_engine_types::{commit_result::TransactionResult, TemplateAddress};
use tari_shutdown::ShutdownSignal;
use tari_template_abi::TemplateDef;

use crate::{notify::Notify, services::WalletEvent};

const LOG_TARGET: &str = "tari::dan_wallet_daemon::services::template_monitor";

pub struct TemplateMonitor<TStore, TNetworkInterface> {
    notify: Notify<WalletEvent>,
    wallet_sdk: DanWalletSdk<TStore, TNetworkInterface>,
    shutdown_signal: ShutdownSignal,
}

impl<TStore, TNetworkInterface> TemplateMonitor<TStore, TNetworkInterface>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError,
{
    pub fn new(
        notify: Notify<WalletEvent>,
        wallet_sdk: DanWalletSdk<TStore, TNetworkInterface>,
        shutdown_signal: ShutdownSignal,
    ) -> Self {
        Self {
            notify,
            wallet_sdk,
            shutdown_signal,
        }
    }

    /// Fetching template definition with retry.
    async fn fetch_template_definition(&self, template_address: TemplateAddress) -> anyhow::Result<TemplateDef> {
        let min_wait_time = Duration::from_millis(100);
        let max_wait_time = Duration::from_secs(5);
        let wait_step = Duration::from_millis(100);
        let mut current_wait_time = min_wait_time;
        let mut template_definition = None;
        let network_interface = self.wallet_sdk.get_network_interface();
        while template_definition.is_none() {
            template_definition = match network_interface
                .fetch_template_definition(template_address)
                .await
                .map_err(|error| TransactionApiError::NetworkInterfaceError(format!("{}", error)))
            {
                Ok(template_def) => Some(template_def),
                Err(error) => {
                    error!(target: LOG_TARGET, "Failed to fetch template definition: {}, retry...", error);
                    tokio::time::sleep(current_wait_time).await;
                    if current_wait_time < max_wait_time {
                        current_wait_time = current_wait_time.add(wait_step);
                    }
                    None
                },
            };
        }

        template_definition.ok_or(anyhow!("Could not fetch template definition"))
    }

    async fn handle_wallet_event(&self, event: WalletEvent) -> anyhow::Result<()> {
        if let WalletEvent::TransactionFinalized(event) = event {
            if matches!(event.status, TransactionStatus::Accepted) {
                if let TransactionResult::Accept(diff) = event.finalize.result {
                    let templates_iter = diff.up_iter().filter_map(|(id, value)| {
                        if let Some(template_address) = id.as_template() {
                            if let Some(template) = value.clone().into_substate_value().as_template() {
                                if let Some(key_index) = self.get_key_index_for_public_key(&template.author) {
                                    return Some((key_index, template_address));
                                }
                            }
                        }
                        None
                    });
                    for (key_index, template_addr) in templates_iter {
                        let template_definition = self.fetch_template_definition(template_addr.as_hash()).await?;
                        if let Err(error) = self
                            .wallet_sdk
                            .template_api()
                            .add_authored_template(key_index, template_addr.as_hash(), template_definition)
                            .await
                        {
                            error!(target: LOG_TARGET, "Error saving template to authored ({template_addr:?}): {}", error);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn get_key_index_for_public_key(&self, author_public_key: &PublicKey) -> Option<u64> {
        if let Ok((key_index, _)) = self
            .wallet_sdk
            .key_manager_api()
            .get_key_for_public_key(key_manager::TRANSACTION_BRANCH, author_public_key)
        {
            return Some(key_index);
        }
        None
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut events_subscription = self.notify.subscribe();
        loop {
            tokio::select! {
                _ = self.shutdown_signal.wait() => {
                    break Ok(());
                }

                Ok(event) = events_subscription.recv() => {
                    if let Err(error) = self.handle_wallet_event(event).await {
                        error!(target: LOG_TARGET, "Error handling event: {}", error);
                    }
                }
            }
        }
    }
}
