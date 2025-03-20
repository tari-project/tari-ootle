// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::error;
use tari_common_types::types::PublicKey;
use tari_dan_common_types::optional::IsNotFoundError;
use tari_dan_wallet_sdk::{
    apis::key_manager,
    models::TransactionStatus,
    network::WalletNetworkInterface,
    storage::WalletStore,
    DanWalletSdk,
};
use tari_engine_types::commit_result::TransactionResult;
use tari_shutdown::ShutdownSignal;

use crate::{notify::Notify, services::WalletEvent};

const LOG_TARGET: &str = "tari::dan::wallet_daemon::template_monitor";

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
                        if let Err(error) = self
                            .wallet_sdk
                            .template_api()
                            .add_authored_template(key_index, template_addr.as_hash())
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
