// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_engine::wasm::WasmModule;
use tari_ootle_common_types::substate_type::SubstateType;
use tari_ootle_wallet_sdk::{WalletSdk, WalletSdkSpec, models::WalletEvent};
use tari_ootle_wallet_sdk_services::notify::Notify;
use tari_shutdown::ShutdownSignal;
use tokio::task;

const LOG_TARGET: &str = "tari::ootle_wallet_daemon::services::template_monitor";

pub struct TemplateMonitor<TSpec: WalletSdkSpec> {
    notify: Notify<WalletEvent>,
    wallet_sdk: WalletSdk<TSpec>,
    shutdown_signal: ShutdownSignal,
}

impl<TSpec> TemplateMonitor<TSpec>
where TSpec: WalletSdkSpec
{
    pub fn new(notify: Notify<WalletEvent>, wallet_sdk: WalletSdk<TSpec>, shutdown_signal: ShutdownSignal) -> Self {
        Self {
            notify,
            wallet_sdk,
            shutdown_signal,
        }
    }

    async fn handle_wallet_event(&self, event: WalletEvent) -> anyhow::Result<()> {
        if let WalletEvent::TransactionFinalized(event) = event {
            let Some(diff) = event.finalize.result.into_any_accept() else {
                return Ok(());
            };

            for (id, substate) in diff.into_up_iter().filter(|(id, _)| id.is_template()) {
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

                let substate_type = SubstateType::from(substate.substate_value());
                let Some(template) = substate.into_substate_value().into_template() else {
                    error!(target: LOG_TARGET, "Diff contained a template substate ID {id} but the substate was type {}. This should not be possible", substate_type);
                    continue;
                };

                match task::spawn_blocking(move || {
                    WasmModule::load_template_from_code(&template.binary).map(|loaded| (template, loaded))
                })
                .await?
                {
                    Ok((template, loaded)) => {
                        self.wallet_sdk.template_api().add_authored_template(
                            template.author,
                            template_address.as_hash(),
                            loaded.template_def().clone(),
                        )?;
                    },
                    Err(err) => {
                        error!(target: LOG_TARGET, "Failed to load template {id} from transaction diff: {}", err);
                        continue;
                    },
                }
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
