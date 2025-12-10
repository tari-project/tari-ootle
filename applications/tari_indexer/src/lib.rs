// Copyright 2023. The Tari Project
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

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

mod bootstrap;
pub mod cli;
pub mod config;
mod dry_run;
pub mod graphql;
#[cfg(feature = "web_ui")]
mod http_ui;
mod rest_api;

mod event;
mod event_manager;
#[cfg(feature = "metrics")]
mod metrics;
mod network_client;
mod network_state_sync;
mod notify;
mod storage_sqlite;
mod store;
mod substate_file_cache;
mod substate_manager;
mod template_manager;
mod transaction_manager;

use std::{convert::Infallible, fs, future, future::Future};

use log::*;
use serde::Serialize;
use tari_common::exit_codes::{ExitCode, ExitError};
use tari_consensus::consensus_constants::ConsensusConstants;
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_epoch_manager::{
    traits::{EpochManagerSpec, LayerOneTransactionSubmitter},
    EpochManagerEvent,
    EpochManagerReader,
};
use tari_epoch_oracles::EpochOracle;
use tari_networking::NetworkingService;
use tari_ootle_app_utilities::keypair::setup_keypair_prompt;
use tari_ootle_common_types::{layer_one_transaction::LayerOneTransactionDef, PeerAddress};
use tari_ootle_storage::global::{DbFactory, GlobalDb};
use tari_ootle_storage_sqlite::{global::SqliteGlobalDbAdapter, SqliteDbFactory};
use tari_shutdown::ShutdownSignal;
use tokio::task;

use crate::{
    bootstrap::{spawn_services, Services},
    config::ApplicationConfig,
    event_manager::EventManager,
    graphql::server::run_graphql,
};

const LOG_TARGET: &str = "tari::indexer::app";

#[allow(clippy::too_many_lines)]
pub async fn run_indexer(config: ApplicationConfig, mut shutdown_signal: ShutdownSignal) -> anyhow::Result<()> {
    info!(target: LOG_TARGET, "Starting indexer node on network {}", config.network);
    let keypair = setup_keypair_prompt(config.to_identity_file_path(), true)?;

    let db_factory = SqliteDbFactory::new(config.global_db_path());
    db_factory
        .migrate()
        .map_err(|e| ExitError::new(ExitCode::DatabaseError, e))?;
    let global_db = db_factory
        .get_or_create_global_db()
        .map_err(|e| ExitError::new(ExitCode::DatabaseError, e))?;

    #[cfg(feature = "metrics")]
    let mut registry = create_metrics_registry(keypair.public_key());

    let consensus_constants = ConsensusConstants::from(config.network);
    let services = spawn_services(
        &config,
        shutdown_signal.clone(),
        keypair.clone(),
        global_db,
        consensus_constants.clone(),
        #[cfg(feature = "metrics")]
        &mut registry,
    )
    .await?;

    // Run the GraphQL API
    let graphql_address = config.indexer.graphql_address;
    if let Some(address) = graphql_address {
        info!(target: LOG_TARGET, "🌐 Started GraphQL server on {}", address);
        task::spawn(run_graphql(
            address,
            services.substate_manager.clone(),
            services.store.clone(),
        ));
    }

    // Run the REST API
    let listen_addr = config.indexer.api_listen_address;
    if let Some(listen_addr) = listen_addr {
        #[cfg(not(feature = "metrics"))]
        let server = rest_api::Server::new();
        #[cfg(feature = "metrics")]
        let server = rest_api::Server::new(registry);
        let listen_address = server
            .spawn(listen_addr, &services, shutdown_signal.clone())
            .await
            .map_err(|e| ExitError::new(ExitCode::ConfigError, e))?;
        debug!(target: LOG_TARGET, "API address {}", listen_address);
        // Run the web ui
        #[cfg(feature = "web_ui")]
        if let Some(address) = config.indexer.web_ui_address {
            let public_api_url = config
                .indexer
                .web_ui_public_api_url
                .unwrap_or_else(|| format!("http://{listen_address}"));
            let public_api_address = url::Url::parse(&public_api_url).map_err(|err| {
                ExitError::new(
                    ExitCode::ConfigError,
                    format!("Invalid public API url '{public_api_url}': {err}"),
                )
            })?;

            // graphql
            let public_graphql_url = config
                .indexer
                .web_ui_public_graphql_url
                .filter(|_| graphql_address.is_some())
                .or_else(|| graphql_address.map(|a| format!("http://{a}")))
                .map(|addr| {
                    url::Url::parse(&addr).map_err(|err| {
                        ExitError::new(
                            ExitCode::ConfigError,
                            format!("Invalid public GraphQL url '{addr}': {err}"),
                        )
                    })
                })
                .transpose()?;

            tokio::spawn(http_ui::server::run_http_ui_server(
                address,
                public_api_address,
                public_graphql_url,
            ));
        }
    }
    #[cfg(not(feature = "web_ui"))]
    info!(target: LOG_TARGET, "🕸️ Web UI not enabled. Compile with --features web_ui to enable it.");

    // Create pid to allow watchers to know that the process has started
    fs::write(config.common.base_path.join("pid"), std::process::id().to_string())
        .map_err(|e| ExitError::new(ExitCode::IOError, e))?;

    // let mut scanning_interval = time::interval(config.indexer.block_scanning_interval);
    // Skip - because we assume that the reason we missed it is because of scanning
    // scanning_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    let mut epoch_manager_events = services.epoch_manager.subscribe();

    loop {
        tokio::select! {
            Ok(event) = epoch_manager_events.recv() => {
                if let Err(err) = handle_epoch_manager_event(&services, event).await {
                    error!(target: LOG_TARGET, "Error handling epoch manager event: {}", err);
                }
            },

            _ = shutdown_signal.wait() => {
                debug!(target: LOG_TARGET, "Shutting down run_substate_polling");
                break;
            },
        }
    }

    shutdown_signal.wait().await;

    Ok(())
}

async fn handle_epoch_manager_event(services: &Services, event: EpochManagerEvent) -> Result<(), anyhow::Error> {
    let EpochManagerEvent::EpochChanged { epoch, .. } = event;
    let all_vns = services.epoch_manager.get_all_validator_nodes(epoch).await?;
    services
        .networking
        .set_want_peers(all_vns.into_iter().map(|vn| vn.address.as_peer_id()))
        .await?;

    Ok(())
}

pub struct IndexerEpochManagerSpec;

impl EpochManagerSpec for IndexerEpochManagerSpec {
    type Addr = PeerAddress;
    type EpochEventOracle = EpochOracle<GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>>;
    type LayerOneSubmitter = Noop;
}

pub struct Noop;

impl LayerOneTransactionSubmitter for Noop {
    type Error = Infallible;
    type Output = ();

    fn submit_transaction<T: Serialize + Send>(
        &self,
        _proof: LayerOneTransactionDef<T>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        future::ready(Ok(()))
    }
}

#[cfg(feature = "metrics")]
fn create_metrics_registry(public_key: &RistrettoPublicKey) -> prometheus_client::registry::Registry {
    use std::borrow::Cow;
    prometheus_client::registry::Registry::with_labels(
        [
            (Cow::Borrowed("app"), Cow::Borrowed("OotleIndexer")),
            (Cow::Borrowed("public_key"), Cow::Owned(public_key.to_string())),
        ]
        .into_iter(),
    )
}
