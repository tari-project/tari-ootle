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
mod http_ui;

mod block_data;
mod event_data;
mod event_manager;
mod event_scanner;
mod json_rpc;
mod substate_manager;
mod substate_storage_sqlite;
mod transaction_manager;

use std::{convert::Infallible, fs, future, future::Future, sync::Arc};

use event_scanner::{EventFilter, EventScanner};
use http_ui::server::run_http_ui_server;
use log::*;
use serde::Serialize;
use substate_manager::SubstateManager;
use tari_common::exit_codes::{ExitCode, ExitError};
use tari_consensus::consensus_constants::ConsensusConstants;
use tari_dan_app_utilities::{
    keypair::setup_keypair_prompt,
    substate_file_cache::SubstateFileCache,
    template_download_queue::TemplateDownloadQueue,
};
use tari_dan_common_types::{layer_one_transaction::LayerOneTransactionDef, Epoch, PeerAddress};
use tari_dan_engine::transaction::TransactionProcessorConfig;
use tari_dan_storage::global::{DbFactory, GlobalDb};
use tari_dan_storage_sqlite::{global::SqliteGlobalDbAdapter, SqliteDbFactory};
use tari_engine_types::confidential::UnclaimedConfidentialOutput;
use tari_epoch_manager::{
    traits::{EpochManagerSpec, EpochUtxoStore, LayerOneTransactionSubmitter},
    EpochManagerEvent,
    EpochManagerReader,
};
use tari_epoch_oracles::EpochOracle;
use tari_indexer_lib::substate_scanner::SubstateScanner;
use tari_networking::NetworkingService;
use tari_shutdown::ShutdownSignal;
use tokio::{task, time};

use crate::{
    bootstrap::{spawn_services, Services},
    config::ApplicationConfig,
    dry_run::processor::DryRunTransactionProcessor,
    event_manager::EventManager,
    graphql::server::run_graphql,
    json_rpc::{spawn_json_rpc, JsonRpcHandlers},
    transaction_manager::TransactionManager,
};

const LOG_TARGET: &str = "tari::indexer::app";

#[allow(clippy::too_many_lines)]
pub async fn run_indexer(config: ApplicationConfig, mut shutdown_signal: ShutdownSignal) -> Result<(), ExitError> {
    info!(target: LOG_TARGET, "Starting indexer node on network {}", config.network);
    let keypair = setup_keypair_prompt(&config.indexer.identity_file, true)?;

    let db_factory = SqliteDbFactory::new(config.indexer.data_dir.clone());
    db_factory
        .migrate()
        .map_err(|e| ExitError::new(ExitCode::DatabaseError, e))?;
    let global_db = db_factory
        .get_or_create_global_db()
        .map_err(|e| ExitError::new(ExitCode::DatabaseError, e))?;

    let consensus_constants = ConsensusConstants::from(config.network);
    let services: Services = spawn_services(
        &config,
        shutdown_signal.clone(),
        keypair.clone(),
        global_db,
        consensus_constants.clone(),
    )
    .await?;

    let mut epoch_manager_events = services.epoch_manager.subscribe();

    let substate_cache_dir = config.common.base_path.join("substate_cache");
    let substate_cache = SubstateFileCache::new(substate_cache_dir)
        .map_err(|e| ExitError::new(ExitCode::ConfigError, format!("Substate cache error: {}", e)))?;

    let dan_layer_scanner = Arc::new(SubstateScanner::new(
        services.epoch_manager.clone(),
        services.validator_node_client_factory.clone(),
        substate_cache,
    ));

    let substate_manager = Arc::new(SubstateManager::new(
        dan_layer_scanner.clone(),
        services.substate_store.clone(),
    ));
    let transaction_manager = TransactionManager::new(
        services.epoch_manager.clone(),
        services.validator_node_client_factory.clone(),
        dan_layer_scanner.clone(),
    );

    // dry run
    let dry_run_transaction_processor = DryRunTransactionProcessor::new(
        TransactionProcessorConfig::builder()
            .with_network(config.network)
            .with_template_binary_max_size_bytes(consensus_constants.template_binary_max_size_bytes)
            .build(),
        services.epoch_manager.clone(),
        services.validator_node_client_factory.clone(),
        dan_layer_scanner.clone(),
        services.template_manager.clone(),
    );

    // Run the JSON-RPC API
    let jrpc_address = config.indexer.json_rpc_address;
    if let Some(jrpc_address) = jrpc_address {
        info!(target: LOG_TARGET, "🌐 Started JSON-RPC server on {}", jrpc_address);
        let handlers = JsonRpcHandlers::new(
            &services,
            substate_manager.clone(),
            transaction_manager,
            services.global_db.clone(),
            services.template_manager.clone(),
            dry_run_transaction_processor,
        );
        let jrpc_address = spawn_json_rpc(jrpc_address, handlers)?;
        // Run the http ui
        if let Some(address) = config.indexer.web_ui_address {
            let mut public_jrpc_address = config
                .indexer
                .ui_connect_address
                .unwrap_or_else(|| jrpc_address.to_string());
            if !public_jrpc_address.starts_with("http://") && !public_jrpc_address.starts_with("https://") {
                public_jrpc_address = format!("http://{}", public_jrpc_address);
            }

            let public_jrpc_address = url::Url::parse(&public_jrpc_address)
                .map_err(|err| ExitError::new(ExitCode::ConfigError, format!("Invalid ui_connect_address: {err}")))?;
            task::spawn(run_http_ui_server(address, public_jrpc_address));
        }
    }

    // Run the event manager
    let event_manager = Arc::new(EventManager::new(
        services.substate_store.clone(),
        dan_layer_scanner.clone(),
    ));

    // Run the event scanner
    let event_filters: Vec<EventFilter> = config
        .indexer
        .event_filters
        .into_iter()
        .map(TryInto::try_into)
        .collect::<Result<_, _>>()
        .map_err(|e| ExitError::new(ExitCode::ConfigError, format!("Invalid event filters: {}", e)))?;
    let event_scanner = EventScanner::new(
        services.epoch_manager.clone(),
        services.validator_node_client_factory.clone(),
        services.substate_store.clone(),
        event_filters,
    );

    // Run the GraphQL API
    let graphql_address = config.indexer.graphql_address;
    if let Some(address) = graphql_address {
        info!(target: LOG_TARGET, "🌐 Started GraphQL server on {}", address);
        task::spawn(run_graphql(address, substate_manager.clone(), event_manager.clone()));
    }

    // Create pid to allow watchers to know that the process has started
    fs::write(config.common.base_path.join("pid"), std::process::id().to_string())
        .map_err(|e| ExitError::new(ExitCode::IOError, e))?;

    loop {
        tokio::select! {
            // keep scanning the dan layer for new events
            _ = time::sleep(config.indexer.dan_layer_scanning_internal) => {
                match event_scanner.scan_events().await {
                    Ok(0) => {},
                    Ok(cnt) => info!(target: LOG_TARGET, "Scanned {} events(s) successfully", cnt),
                    Err(e) =>  error!(target: LOG_TARGET, "Event auto-scan failed: {}", e),
                };
            },

            Ok(event) = epoch_manager_events.recv() => {
                if let Err(err) = handle_epoch_manager_event(&services, event).await {
                    error!(target: LOG_TARGET, "Error handling epoch manager event: {}", err);
                }
            },

            _ = shutdown_signal.wait() => {
                dbg!("Shutting down run_substate_polling");
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
    type TemplateDownloader = TemplateDownloadQueue;
    type UtxoStore = Noop;
}

pub struct Noop;

impl EpochUtxoStore for Noop {
    type Error = Infallible;

    fn add_unclaimed_utxo(&mut self, _epoch: Epoch, _substate: UnclaimedConfidentialOutput) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl LayerOneTransactionSubmitter for Noop {
    type Error = Infallible;

    fn submit_transaction<T: Serialize + Send>(
        &self,
        _proof: LayerOneTransactionDef<T>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        future::ready(Ok(()))
    }
}
