// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{ops::Add, time::Duration};

use anyhow::anyhow;
use log::*;
use tari_ootle_common_types::{
    optional::{IsNotFoundError, Optional},
    substate_type::SubstateType,
};
use tari_ootle_wallet_sdk::{
    apis::transaction::TransactionApiError,
    network::WalletNetworkInterface,
    storage::WalletStore,
    WalletSdk,
};
use tari_shutdown::ShutdownSignal;
use tari_template_abi::TemplateDef;
use tari_template_lib::types::TemplateAddress;

use crate::{notify::Notify, services::WalletEvent};

const LOG_TARGET: &str = "tari::ootle_wallet_daemon::services::template_monitor";

pub struct TemplateMonitor<TStore, TNetworkInterface> {
    notify: Notify<WalletEvent>,
    wallet_sdk: WalletSdk<TStore, TNetworkInterface>,
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
        wallet_sdk: WalletSdk<TStore, TNetworkInterface>,
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
        let network_interface = self.wallet_sdk.get_network_interface();
        loop {
            match network_interface
                .fetch_template_definition(template_address)
                .await
                .optional()
                .map_err(|error| TransactionApiError::NetworkInterfaceError(error.to_string()))?
            {
                Some(template_def) => {
                    return Ok(template_def);
                },
                None => {
                    info!(target: LOG_TARGET, "Template definition not found yet. retry after {:.2?}...", current_wait_time);
                    if self.shutdown_signal.is_triggered() {
                        return Err(anyhow!("shutdown during fetch template definition"));
                    }
                    tokio::time::sleep(current_wait_time).await;
                    if current_wait_time < max_wait_time {
                        current_wait_time = current_wait_time.add(wait_step);
                    }
                },
            };
        }
    }

    async fn handle_wallet_event(&self, event: WalletEvent) -> anyhow::Result<()> {
        if let WalletEvent::TransactionFinalized(event) = event {
            let Some(diff) = event.finalize.result.accept() else {
                return Ok(());
            };

            for (id, substate) in diff.up_iter().filter(|(id, _)| id.is_template()) {
                let template_address = id
                    .as_template()
                    .expect("is_template checked but as_template returned None");
                if self
                    .wallet_sdk
                    .template_api()
                    .template_exists(template_address.as_hash())?
                {
                    // Template already exists, no need to add it again
                    info!(target: LOG_TARGET, "Template {id} already exists, skipping...");
                    continue;
                }

                let Some(template) = substate.substate_value().as_template() else {
                    error!(target: LOG_TARGET, "Diff contained a template substate ID {id} but the substate was type {}. This should not be possible", SubstateType::from(substate.substate_value()));
                    continue;
                };
                let template_definition = match self.fetch_template_definition(template_address.as_hash()).await {
                    Ok(template_definition) => template_definition,
                    Err(error) => {
                        error!(target: LOG_TARGET, "Failed to fetch template definition: {}", error);
                        continue;
                    },
                };
                self.wallet_sdk.template_api().add_authored_template(
                    template.author,
                    template_address.as_hash(),
                    template_definition,
                )?;
            }
        }
        Ok(())
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
