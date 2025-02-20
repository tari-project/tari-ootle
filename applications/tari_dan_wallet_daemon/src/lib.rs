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
mod http_ui;
pub mod indexer_jrpc_impl;
mod jrpc_server;
mod notify;
mod services;
mod webrtc;

use std::{fs, panic, process, sync::Arc, time::Duration};

use log::*;
use tari_dan_common_types::{optional::Optional, NumPreshards};
use tari_dan_wallet_sdk::{
    apis::{
        config::{ConfigApi, ConfigKey},
        key_manager,
    },
    DanWalletSdk,
    WalletSdkConfig,
};
use tari_dan_wallet_storage_sqlite::SqliteWalletStore;
use tari_shutdown::ShutdownSignal;
use tari_template_lib::models::Amount;
use tokio::task;
use url::Url;
use webauthn_rs::WebauthnBuilder;

use crate::{
    config::{ApplicationConfig, WalletDaemonConfig},
    handlers::{auth::get_authenticator, HandlerContext},
    http_ui::server::run_http_ui_server,
    indexer_jrpc_impl::IndexerJsonRpcNetworkInterface,
    notify::Notify,
    services::{spawn_services, WebauthnService},
};

const LOG_TARGET: &str = "tari::dan::wallet_daemon";

const DEFAULT_FEE: Amount = Amount::new(1500);
// TODO: must match the global network value. All testnets currently have 256 pre-shards.
const NUM_PRESHARDS: NumPreshards = NumPreshards::current();

pub async fn run_tari_dan_wallet_daemon(
    config: ApplicationConfig,
    shutdown_signal: ShutdownSignal,
) -> Result<(), anyhow::Error> {
    // Uncomment to enable tokio tracing via tokio-console
    // console_subscriber::init();

    let wallet_store = init_wallet_store(&config)?;

    let wallet_sdk = initialize_wallet_sdk(&config, wallet_store.clone())?;
    wallet_sdk
        .key_manager_api()
        .get_or_create_initial(key_manager::TRANSACTION_BRANCH)?;
    let notify = Notify::new(100);

    let services = spawn_services(shutdown_signal.clone(), notify.clone(), wallet_sdk.clone());

    let jrpc_address = config.dan_wallet_daemon.json_rpc_address.unwrap();
    let signaling_server_address = config.dan_wallet_daemon.signaling_server_address.unwrap();
    let webauthn_service = Arc::new(WebauthnService::new(wallet_store, Duration::from_secs(60 * 60)));

    // webauthn
    let rp_origin = match config.dan_wallet_daemon.web_ui_address {
        Some(ui_address) => Url::parse(format!("http://localhost:{}", ui_address.port()).as_str())?,
        None => {
            let ui_address = WalletDaemonConfig::default().web_ui_address.unwrap();
            Url::parse(format!("http://localhost:{}", ui_address.port()).as_str())?
        },
    };
    let webauthn = WebauthnBuilder::new("localhost", &rp_origin)?
        .rp_name("Tari Ootle")
        .build()?;

    let authenticator = get_authenticator(
        config.dan_wallet_daemon.authentication.clone(),
        webauthn.clone(),
        webauthn_service.clone(),
    );

    let handlers = HandlerContext::new(
        wallet_sdk.clone(),
        notify,
        services.transaction_service_handle.clone(),
        services.account_monitor_handle.clone(),
        config.dan_wallet_daemon.clone(),
        authenticator,
        webauthn_service,
        webauthn,
    );
    let (jrpc_address, listen_fut) =
        jrpc_server::spawn_listener(jrpc_address, signaling_server_address, handlers, shutdown_signal)?;

    // Run the http ui
    if let Some(web_listener_address) = config.dan_wallet_daemon.web_ui_address {
        let mut public_jrpc_url = config
            .dan_wallet_daemon
            .web_ui_public_json_rpc_url
            .unwrap_or_else(|| jrpc_address.to_string());
        if !public_jrpc_url.starts_with("http://") && !public_jrpc_url.starts_with("https://") {
            public_jrpc_url = format!("http://{}", public_jrpc_url);
        }

        let public_jrpc_url = url::Url::parse(&public_jrpc_url)?;
        task::spawn(run_http_ui_server(web_listener_address, public_jrpc_url));
    }

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
    config: &ApplicationConfig,
    store: SqliteWalletStore,
) -> anyhow::Result<DanWalletSdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface>> {
    let sdk_config = WalletSdkConfig {
        // TODO: Configure
        password: None,
        jwt_expiry: config.dan_wallet_daemon.jwt_expiry.unwrap(),
        jwt_secret_key: config.dan_wallet_daemon.jwt_secret_key.clone().unwrap(),
    };
    let config_api = ConfigApi::new(&store);
    let indexer_jrpc_endpoint = if let Some(indexer_url) = config_api.get(ConfigKey::IndexerUrl).optional()? {
        indexer_url
    } else {
        config.dan_wallet_daemon.indexer_json_rpc_url.clone()
    };
    let indexer = IndexerJsonRpcNetworkInterface::new(indexer_jrpc_endpoint);
    let wallet_sdk = DanWalletSdk::initialize(config.dan_wallet_daemon.network, store, indexer, sdk_config)?;
    Ok(wallet_sdk)
}
