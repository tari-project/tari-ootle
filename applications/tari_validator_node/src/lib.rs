// Copyright 2022. The Tari Project
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

mod bootstrap;
pub mod cli;
mod config;
mod consensus;
mod dan_node;
mod dry_run_transaction_processor;
mod event_subscription;
#[cfg(feature = "web_ui")]
mod http_ui;
mod json_rpc;
#[cfg(feature = "metrics")]
mod metrics;
mod p2p;
mod substate_resolver;

mod file_l1_submitter;
mod state_bootstrap;
pub mod transaction_validators;
mod validator;

use std::{fs, io, process};

use log::*;
use serde::{Deserialize, Serialize};
use tari_common::exit_codes::{ExitCode, ExitError};
use tari_consensus::consensus_constants::ConsensusConstants;
use tari_dan_app_utilities::{
    keypair::setup_keypair_prompt,
    template_download_queue::TemplateDownloadQueue,
    utxo_store::StateUtxoStore,
};
use tari_dan_common_types::{PeerAddress, SubstateAddress};
use tari_dan_storage::global::{DbFactory, GlobalDb};
use tari_dan_storage_sqlite::{global::SqliteGlobalDbAdapter, SqliteDbFactory};
use tari_epoch_manager::traits::EpochManagerSpec;
use tari_epoch_oracles::EpochOracle;
use tari_shutdown::Shutdown;

pub use crate::config::{ApplicationConfig, ValidatorNodeConfig};
use crate::{
    bootstrap::{spawn_services, Services},
    consensus::spec::ValidatorNodeStateStore,
    dan_node::DanNode,
    file_l1_submitter::FileLayerOneSubmitter,
    json_rpc::{spawn_json_rpc, JsonRpcHandlers},
};

const LOG_TARGET: &str = "tari::validator_node::app";

#[derive(Debug, thiserror::Error)]
pub enum ShardKeyError {
    #[error("Path is not a file")]
    NotFile,
    #[error("Malformed shard key file: {0}")]
    JsonError(#[from] json5::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("Not yet mined")]
    NotYetMined,
    #[error("Not yet registered")]
    NotYetRegistered,
    #[error("Registration failed")]
    RegistrationFailed,
}

#[derive(Serialize, Deserialize)]
pub struct ShardKey {
    is_registered: bool,
    substate_address: Option<SubstateAddress>,
}

pub async fn run_validator_node(config: ApplicationConfig, shutdown: Shutdown) -> Result<(), anyhow::Error> {
    info!(target: LOG_TARGET, "Starting validator node on network {}", config.network);
    let keypair = setup_keypair_prompt(
        &config.validator_node.identity_file,
        !config.validator_node.dont_create_id,
    )?;

    let db_factory = SqliteDbFactory::new(config.validator_node.data_dir.clone());
    db_factory
        .migrate()
        .map_err(|e| ExitError::new(ExitCode::DatabaseError, e))?;
    let global_db = db_factory
        .get_or_create_global_db()
        .map_err(|e| ExitError::new(ExitCode::DatabaseError, e))?;

    info!(
        target: LOG_TARGET,
        "🚀 Node starting with pub key: {} and peer id {}",
        keypair.public_key(),keypair.to_peer_address(),
    );

    #[cfg(feature = "metrics")]
    let metrics_registry = create_metrics_registry(keypair.public_key());

    let consensus_constants = ConsensusConstants::from(config.network);
    let services = spawn_services(
        config.clone(),
        shutdown.to_signal(),
        keypair.clone(),
        global_db,
        consensus_constants,
        #[cfg(feature = "metrics")]
        &metrics_registry,
    )
    .await?;
    let info = services.networking.get_local_peer_info().await?;
    info!(target: LOG_TARGET, "🚀 Node started: {}", info);

    // Run the JSON-RPC API
    let mut jrpc_address = config.validator_node.json_rpc_listener_address;
    if let Some(jrpc_address) = jrpc_address.as_mut() {
        info!(target: LOG_TARGET, "🌐 Started JSON-RPC server on {}", jrpc_address);
        let handlers = JsonRpcHandlers::new(&services);
        *jrpc_address = spawn_json_rpc(
            *jrpc_address,
            handlers,
            #[cfg(feature = "metrics")]
            metrics_registry,
        )?;
        // Run the web ui
        #[cfg(feature = "web_ui")]
        if let Some(address) = config.validator_node.web_ui_listener_address {
            tokio::spawn(http_ui::server::run_http_ui_server(
                address,
                config
                    .validator_node
                    .json_rpc_public_url
                    .clone()
                    .unwrap_or_else(|| jrpc_address.to_string()),
            ));
        }
    }
    #[cfg(not(feature = "web_ui"))]
    info!(
        target: LOG_TARGET,
        "Web UI is not enabled. To enable it, add the `web_ui` feature to your Cargo.toml"
    );

    fs::write(config.common.base_path.join("pid"), process::id().to_string())
        .map_err(|e| ExitError::new(ExitCode::UnknownError, e))?;
    let node = DanNode::new(services);
    info!(target: LOG_TARGET, "🚀 Validator node started!");
    node.start(shutdown)
        .await
        .map_err(|e| ExitError::new(ExitCode::UnknownError, e))?;

    Ok(())
}

#[cfg(feature = "metrics")]
fn create_metrics_registry(public_key: &tari_crypto::ristretto::RistrettoPublicKey) -> prometheus::Registry {
    let mut labels = std::collections::HashMap::with_capacity(2);
    labels.insert("app".to_string(), "ValidatorNode".to_string());
    labels.insert("public_key".to_string(), public_key.to_string());
    prometheus::Registry::new_custom(Some("tari".to_string()), Some(labels)).unwrap()
}

pub struct ValidatorNodeEpochManagerSpec;

impl EpochManagerSpec for ValidatorNodeEpochManagerSpec {
    type Addr = PeerAddress;
    type EpochEventOracle = EpochOracle<GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>>;
    type LayerOneSubmitter = FileLayerOneSubmitter;
    type TemplateDownloader = TemplateDownloadQueue;
    type UtxoStore = StateUtxoStore<ValidatorNodeStateStore>;
}
