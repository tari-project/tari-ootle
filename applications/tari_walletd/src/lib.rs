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
mod server;
mod services;
mod webrtc;

use std::{fs, panic, pin, process};

use jsonwebtoken::signature::rand_core::OsRng;
use log::*;
use tari_common_types::seeds::seed_words::SeedWords;
use tari_crypto::{keys::SecretKey, ristretto::RistrettoSecretKey};
use tari_ootle_app_utilities::genesis_resources::{get_public_identity_resource, get_stealth_tari_resource};
use tari_ootle_common_types::{Network, NumPreshards, optional::Optional};
use tari_ootle_wallet_sdk::{
    WalletSdk as Sdk,
    WalletSdkConfig,
    WalletSdkSpec,
    apis::config::{ConfigApi, ConfigKey},
    cipher_seed::CipherSeedRestore,
    local_key_store::LocalKeyStore,
    models::EpochBirthday,
};
use tari_ootle_wallet_sdk_services::{
    account_recovery::AccountRecoveryService,
    indexer_rest_api::IndexerRestApiNetworkInterface,
    notify::Notify,
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_shutdown::ShutdownSignal;
use tari_utilities::{SafePassword, hex::Hex};
use url::Url;

use crate::{
    config::ApplicationConfig,
    handlers::{HandlerContext, auth::create_authenticator},
    services::spawn_services,
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon";

const DEFAULT_FEE: u64 = 1500;
// TODO: must match the global network value. All testnets currently have 256 pre-shards.
const NUM_PRESHARDS: NumPreshards = NumPreshards::current();

pub struct OotleWalletDaemonSpec;

impl WalletSdkSpec for OotleWalletDaemonSpec {
    type KeyStore = LocalKeyStore;
    type NetworkInterface = IndexerRestApiNetworkInterface;
    type Store = SqliteWalletStore;
}

pub type WalletSdk = Sdk<OotleWalletDaemonSpec>;

pub async fn run_tari_ootle_walletd(
    config: ApplicationConfig,
    seed_words: Option<&SeedWords>,
    shutdown_signal: ShutdownSignal,
) -> Result<(), anyhow::Error> {
    // Uncomment to enable tokio tracing via tokio-console
    // console_subscriber::init();

    let wallet_store = init_wallet_store(&config)?;
    let mut wallet_sdk: WalletSdk = initialize_wallet_sdk(&config, wallet_store.clone())?;

    info!(
        target: LOG_TARGET,
        "🟢 Starting wallet on {} connected to indexer {}",
        wallet_sdk.network(),
        wallet_sdk
            .config_api()
            .get::<Url>(ConfigKey::IndexerUrl)
            .optional()?
            .unwrap_or_else(|| config.ootle_wallet_daemon.indexer_api_url.clone())
    );

    let needs_seed_recovery =
        wallet_sdk.initialize_cipher_seed(seed_words.map(CipherSeedRestore::FromSeedWords).unwrap_or_default())?;

    // Insert genesis resources
    let (xtr_addr, xtr_resx) = get_stealth_tari_resource(wallet_sdk.network());
    wallet_sdk.resources_api().upsert_resource(&xtr_addr, &xtr_resx)?;
    let (addr, resx) = get_public_identity_resource();
    wallet_sdk.resources_api().upsert_resource(&addr, &resx)?;

    let notify = Notify::new(100);
    let burn_proof_dir = config.ootle_wallet_daemon.get_burn_proof_dir(wallet_sdk.network());
    let services = spawn_services(
        shutdown_signal.clone(),
        notify.clone(),
        wallet_sdk.clone(),
        burn_proof_dir,
    );

    // trigger account scanning if needed
    if needs_seed_recovery {
        let cipher_seed_birthday = wallet_sdk.key_manager_api().get_cipher_seed_birthday_epoch()?;
        let scanner = AccountRecoveryService::new(
            wallet_sdk.clone(),
            services.account_monitor_handle.clone(),
            config.ootle_wallet_daemon.recovery_abandon_count,
            cipher_seed_birthday,
        );
        let shutdown_signal = shutdown_signal.clone();
        tokio::spawn(async move {
            let scan_pinned = pin::pin!(scanner.scan());
            shutdown_signal.select(scan_pinned).await;
        });
    }

    let jrpc_address = config.ootle_wallet_daemon.json_rpc_address;
    let signaling_server_address = config.ootle_wallet_daemon.signaling_server_address.unwrap();

    // webauthn

    info!(
        target: LOG_TARGET,
        "🔐 Authentication method set to {:?}",
        config.ootle_wallet_daemon.authentication
    );
    let authenticator = create_authenticator(&config.ootle_wallet_daemon, wallet_store.clone())?;

    // Generate a new secret each time the daemon starts. This means that all JWT tokens will be invalidated on restart.
    let jwt_secret = create_secret_password();
    let handlers = HandlerContext::new(
        wallet_sdk.clone(),
        notify,
        services.transaction_service_handle.clone(),
        services.account_monitor_handle.clone(),
        config.ootle_wallet_daemon.clone(),
        authenticator,
        jwt_secret,
        shutdown_signal,
    );
    let (_jrpc_address, listen_fut) = server::spawn_listener(jrpc_address, signaling_server_address, handlers).await?;

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
            match res {
                Ok(_) => {
                    info!(target: LOG_TARGET, "All services have shut down");
                },
                Err(err) => {
                    error!(target: LOG_TARGET, "🚨 A service has crashed: {}. Shutting down", err);
                    return Err(err);
                },
            }
        },
    }
    Ok(())
}

pub fn init_wallet_store(config: &ApplicationConfig) -> anyhow::Result<SqliteWalletStore> {
    let store = SqliteWalletStore::try_open(config.to_data_dir().join("wallet.sqlite"))?;
    store.run_migrations()?;
    Ok(store)
}

pub fn initialize_wallet_sdk(config: &ApplicationConfig, store: SqliteWalletStore) -> anyhow::Result<WalletSdk> {
    let sdk_config = WalletSdkConfig {
        network: config.ootle_wallet_daemon.network,
        override_keyring_password: config.ootle_wallet_daemon.override_keyring_password.clone(),
    };
    let config_api = ConfigApi::new(&store);
    let indexer_endpoint = if let Some(indexer_url) = config_api.get(ConfigKey::IndexerUrl).optional()? {
        indexer_url
    } else {
        config.ootle_wallet_daemon.indexer_api_url.clone()
    };
    let indexer = IndexerRestApiNetworkInterface::new(indexer_endpoint);
    let birthday = get_epoch_birthday(sdk_config.network);
    let sdk = WalletSdk::initialize_with_local_key_store(store, indexer, sdk_config, birthday)?;
    Ok(sdk)
}

const fn get_epoch_birthday(network: Network) -> EpochBirthday {
    // TODO: set the zero epoch time for each network according to actual zero epoch time
    match network {
        Network::MainNet => EpochBirthday::far_future(),
        Network::StageNet => EpochBirthday::far_future(),
        Network::NextNet => EpochBirthday::far_future(),
        Network::LocalNet => EpochBirthday::far_future(),
        Network::Igor => EpochBirthday::far_future(),
        Network::Esmeralda => EpochBirthday::far_future(),
    }
}

fn create_secret_password() -> SafePassword {
    let secret = RistrettoSecretKey::random(&mut OsRng);
    // It is safe to use to_hex() since the String's underlying Vec is moved directly into SafePassword
    SafePassword::from(secret.to_hex())
}
