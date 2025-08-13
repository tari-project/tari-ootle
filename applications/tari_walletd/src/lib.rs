// Copyright 2021. The Tari Project
//
// Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
// following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
// disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
// following disclaimer in the documentation and/or other materials provided with the distribution.
//
// 3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
// products derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
// WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
// USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

pub mod cli;
pub mod config;
mod handlers;
#[cfg(feature = "web_ui")]
mod http_ui;
pub mod indexer_jrpc_impl;
mod jrpc_server;
mod notify;
mod services;
mod webrtc;

use std::{fs, panic, process};

use log::*;
use tari_ootle_common_types::{optional::Optional, NumPreshards};
use tari_ootle_wallet_sdk::{
    apis::{
        config::{ConfigApi, ConfigKey},
        key_manager::KeyBranch,
    },
    WalletSdk,
    WalletSdkConfig,
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_shutdown::ShutdownSignal;

use crate::{
    cli::Cli,
    config::ApplicationConfig,
    handlers::{auth::create_authenticator, HandlerContext},
    indexer_jrpc_impl::IndexerJsonRpcNetworkInterface,
    notify::Notify,
    services::{recovery_service, spawn_services},
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon";

const DEFAULT_FEE: u64 = 1500;
// TODO: must match the global network value. All testnets currently have 256 pre-shards.
const NUM_PRESHARDS: NumPreshards = NumPreshards::current();

pub async fn run_tari_ootle_walletd(
    cli: Cli,
    config: ApplicationConfig,
    shutdown_signal: ShutdownSignal,
) -> Result<(), anyhow::Error> {
    // Uncomment to enable tokio tracing via tokio-console
    // console_subscriber::init();

    let wallet_store = init_wallet_store(&config)?;
    let mut wallet_sdk = initialize_wallet_sdk(&cli, &config, wallet_store.clone())?;

    let needs_seed_recovery = wallet_sdk.initialize_cipher_seed(cli.wallet_restore.seed_words.as_ref())?;

    wallet_sdk.key_manager_api().get_or_create_initial(KeyBranch::Account)?;

    let notify = Notify::new(100);
    let services = spawn_services(shutdown_signal.clone(), notify.clone(), wallet_sdk.clone());

    // trigger resource scanning if needed
    if needs_seed_recovery {
        let scanner = recovery_service::Service::new(
            wallet_sdk.clone(),
            services.account_monitor_handle.clone(),
            config.ootle_wallet_daemon.recovery_abandon_count,
            shutdown_signal.clone(),
        );
        tokio::spawn(scanner.scan());
    }

    let jrpc_address = config.ootle_wallet_daemon.json_rpc_address.unwrap();
    let signaling_server_address = config.ootle_wallet_daemon.signaling_server_address.unwrap();

    // webauthn

    let authenticator = create_authenticator(&config.ootle_wallet_daemon, wallet_store.clone())?;

    let handlers = HandlerContext::new(
        wallet_sdk.clone(),
        notify,
        services.transaction_service_handle.clone(),
        services.account_monitor_handle.clone(),
        config.ootle_wallet_daemon.clone(),
        authenticator,
        shutdown_signal,
    );
    let (jrpc_address, listen_fut) = jrpc_server::spawn_listener(jrpc_address, signaling_server_address, handlers)?;

    // Run the web ui
    #[cfg(feature = "web_ui")]
    if let Some(web_listener_address) = config.ootle_wallet_daemon.web_ui_address {
        let mut public_jrpc_url = config
            .ootle_wallet_daemon
            .web_ui_public_json_rpc_url
            .unwrap_or_else(|| jrpc_address.to_string());
        if !public_jrpc_url.starts_with("http://") && !public_jrpc_url.starts_with("https://") {
            public_jrpc_url = format!("http://{}", public_jrpc_url);
        }

        let public_jrpc_url = url::Url::parse(&public_jrpc_url)?;

        tokio::spawn(http_ui::server::run_http_ui_server(
            web_listener_address,
            public_jrpc_url,
        ));
    }
    #[cfg(not(feature = "web_ui"))]
    info!(
        target: LOG_TARGET,
        "Web UI is not enabled. To enable it, add the `web_ui` feature to your Cargo.toml. JSON-RPC address is {jrpc_address}"
    );

    if let Err(e) = fs::write(config.common.base_path.join("pid"), process::id().to_string()) {
        error!(
            target: LOG_TARGET,
            "Failed to create PID file {}: {}",
            config.common.base_path.join("pid").display(),
            e
        )
    }
    // Wait for shutdown, or for any service to error
    tokio::select! {
        res = listen_fut => {
            res??;
        },
        res = services.services_fut => {
            res?;
        },
    }
    Ok(())
}

pub fn init_wallet_store(config: &ApplicationConfig) -> anyhow::Result<SqliteWalletStore> {
    let store = SqliteWalletStore::try_open(config.common.base_path.join("data/wallet.sqlite"))?;
    store.run_migrations()?;
    Ok(store)
}

pub fn initialize_wallet_sdk(
    cli: &Cli,
    config: &ApplicationConfig,
    store: SqliteWalletStore,
) -> anyhow::Result<WalletSdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface>> {
    let sdk_config = WalletSdkConfig {
        network: config.ootle_wallet_daemon.network,
        override_keyring_password: cli.override_keyring_password.clone(),
    };
    let config_api = ConfigApi::new(&store);
    let indexer_jrpc_endpoint = if let Some(indexer_url) = config_api.get(ConfigKey::IndexerUrl).optional()? {
        indexer_url
    } else {
        config.ootle_wallet_daemon.indexer_json_rpc_url.clone()
    };
    let indexer = IndexerJsonRpcNetworkInterface::new(indexer_jrpc_endpoint);
    let sdk = WalletSdk::initialize(store, indexer, sdk_config)?;
    Ok(sdk)
}
