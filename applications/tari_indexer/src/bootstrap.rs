//   Copyright 2023. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::{fs, io, str::FromStr};

use anyhow::Context;
use libp2p::identity;
use minotari_app_utilities::identity_management;
use tari_base_node_client::grpc::GrpcBaseNodeClient;
use tari_common::{
    configuration::bootstrap::{grpc_default_port, ApplicationType},
    exit_codes::{ExitCode, ExitError},
};
use tari_consensus::consensus_constants::ConsensusConstants;
use tari_crypto::tari_utilities::ByteArray;
use tari_dan_app_utilities::{
    common::verify_correct_network,
    epoch_oracle_config::EpochOracleType,
    keypair::RistrettoKeypair,
    seed_peer::SeedPeer,
    template_download_queue::TemplateDownloadQueue,
};
use tari_dan_common_types::PeerAddress;
use tari_dan_p2p::TariMessagingSpec;
use tari_dan_storage::global::GlobalDb;
use tari_dan_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_engine_types::ToByteType;
use tari_epoch_manager::service::{EpochManagerConfig, EpochManagerHandle};
use tari_epoch_oracles::{
    base_layer::BaseLayerOracle,
    configured::{ConfiguredEpochOracle, IntervalEpochTicker},
    hybrid::{watch_ticker, HybridEpochOracle},
    store::EpochOracleStore,
    EpochOracle,
};
use tari_networking::{MessagingMode, NetworkingHandle, RelayCircuitLimits, RelayReservationLimits, SwarmConfig};
use tari_shutdown::ShutdownSignal;
use tari_template_lib::prelude::RistrettoPublicKeyBytes;
use tari_template_manager::{implementation::TemplateManager, interface::TemplateManagerHandle};
use tari_validator_node_rpc::client::TariValidatorNodeRpcClientFactory;

use crate::{
    network_client::TariNetworkClient,
    substate_storage_sqlite::sqlite_substate_store_factory::SqliteSubstateStore,
    ApplicationConfig,
    IndexerEpochManagerSpec,
    Noop,
};

const _LOG_TARGET: &str = "tari_indexer::bootstrap";

#[allow(clippy::too_many_lines)]
pub async fn spawn_services(
    config: &ApplicationConfig,
    shutdown: ShutdownSignal,
    keypair: RistrettoKeypair,
    global_db: GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>,
    consensus_constants: ConsensusConstants,
) -> Result<Services, anyhow::Error> {
    ensure_directories_exist(config)?;

    // Initialize networking
    let identity = identity::Keypair::sr25519_from_bytes(keypair.secret_key().as_bytes().to_vec()).map_err(|e| {
        ExitError::new(
            ExitCode::ConfigError,
            format!("Failed to create libp2p identity from secret bytes: {}", e),
        )
    })?;
    let seed_peers = config
        .peer_seeds
        .peer_seeds
        .iter()
        .map(|s| SeedPeer::from_str(s))
        .collect::<anyhow::Result<Vec<SeedPeer>>>()?;
    let seed_peers = seed_peers
        .into_iter()
        .flat_map(|p| {
            let peer_id = p.to_peer_id();
            p.addresses.into_iter().map(move |a| (peer_id, a))
        })
        .collect();
    let (networking, _) = tari_networking::spawn::<TariMessagingSpec>(
        identity,
        MessagingMode::Disabled,
        tari_networking::Config {
            listener_port: config.indexer.p2p.listener_port,
            swarm: SwarmConfig {
                protocol_version: format!("/tari/{}/0.0.1", config.network).parse().unwrap(),
                user_agent: "/tari/indexer/0.0.1".to_string(),
                enable_mdns: config.indexer.p2p.enable_mdns,
                enable_relay: true,
                relay_circuit_limits: RelayCircuitLimits::high(),
                relay_reservation_limits: RelayReservationLimits::high(),
                ..Default::default()
            },
            reachability_mode: config.indexer.p2p.reachability_mode.into(),
            announce: false,
            ..Default::default()
        },
        seed_peers,
        shutdown.clone(),
    )?;

    // Connect to substate db
    let substate_store = SqliteSubstateStore::try_create(config.indexer.state_db_path())?;

    // Epoch event oracle
    let epoch_event_oracle = create_epoch_oracle(config, global_db.clone(), &consensus_constants).await?;

    let (template_queue_sender, template_queue_receiver) = TemplateDownloadQueue::create();

    // Epoch manager
    let (epoch_manager, _) = tari_epoch_manager::service::spawn_service::<IndexerEpochManagerSpec>(
        EpochManagerConfig {
            num_preshards: consensus_constants.num_preshards,
            base_layer_confirmations: consensus_constants.base_layer_confirmations,
            committee_size: consensus_constants
                .committee_size
                .try_into()
                .context("committee_size must be non-zero")?,
            validator_node_sidechain_id: config.indexer.sidechain_id.as_ref().map(|pk| pk.to_byte_type()),
            // N/A
            fee_claim_public_key: RistrettoPublicKeyBytes::zero(),
        },
        global_db.clone(),
        keypair.public_key().to_byte_type(),
        epoch_event_oracle,
        Noop,
        template_queue_sender,
        Noop,
        shutdown.clone(),
    );

    // Client factory
    let validator_node_client_factory = TariValidatorNodeRpcClientFactory::new(networking.clone());
    let network_client = TariNetworkClient::new(epoch_manager.clone(), validator_node_client_factory.clone());

    // Template manager
    let template_manager = TemplateManager::initialize(global_db.clone(), config.indexer.templates.clone())?;
    let (template_manager_service, _) = tari_template_manager::implementation::spawn(
        template_manager.clone(),
        epoch_manager.clone(),
        validator_node_client_factory.clone(),
        template_queue_receiver,
        shutdown.clone(),
    );

    // Save final node identity after comms has initialized. This is required because the public_address can be
    // changed by comms during initialization when using tor.
    save_identities(config, &keypair)?;
    Ok(Services {
        keypair,
        networking,
        epoch_manager,
        validator_node_client_factory,
        network_client,
        substate_store,
        global_db,
        template_manager,
        template_manager_service,
    })
}

pub struct Services {
    pub keypair: RistrettoKeypair,
    pub networking: NetworkingHandle<TariMessagingSpec>,
    pub epoch_manager: EpochManagerHandle<PeerAddress>,
    pub validator_node_client_factory: TariValidatorNodeRpcClientFactory,
    pub substate_store: SqliteSubstateStore,
    pub network_client: TariNetworkClient<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory>,
    pub global_db: GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>,
    pub template_manager: TemplateManager<PeerAddress>,
    pub template_manager_service: TemplateManagerHandle,
}

fn ensure_directories_exist(config: &ApplicationConfig) -> io::Result<()> {
    fs::create_dir_all(&config.indexer.data_dir)?;
    Ok(())
}

fn save_identities(config: &ApplicationConfig, identity: &RistrettoKeypair) -> Result<(), ExitError> {
    identity_management::save_as_json(&config.indexer.identity_file, identity)
        .map_err(|e| ExitError::new(ExitCode::ConfigError, format!("Failed to save node identity: {}", e)))?;

    Ok(())
}

async fn create_base_layer_client(config: &ApplicationConfig) -> Result<GrpcBaseNodeClient, ExitError> {
    let url = config
        .epoch_oracle
        .base_layer
        .base_node_grpc_url
        .clone()
        .unwrap_or_else(|| {
            let port = grpc_default_port(ApplicationType::BaseNode, config.network);
            format!("http://127.0.0.1:{port}")
                .parse()
                .expect("Default base node GRPC URL is malformed")
        });
    GrpcBaseNodeClient::connect(url)
        .await
        .map_err(|err| ExitError::new(ExitCode::ConfigError, format!("Could not connect to base node {}", err)))
}

async fn create_epoch_oracle<TStore: EpochOracleStore + Clone + Send + 'static>(
    config: &ApplicationConfig,
    store: TStore,
    consensus_constants: &ConsensusConstants,
) -> Result<EpochOracle<TStore>, ExitError> {
    match config.epoch_oracle.oracle_type {
        EpochOracleType::BaseLayer => {
            let oracle = create_base_layer_epoch_oracle(config, store, consensus_constants).await?;
            Ok(EpochOracle::BaseLayer(oracle))
        },
        EpochOracleType::Configured => {
            let oracle = create_configured_epoch_oracle(config, store).await?;
            Ok(EpochOracle::Configured(oracle))
        },
        EpochOracleType::Hybrid => {
            let oracle = create_hybrid_epoch_oracle(config, store, consensus_constants).await?;
            Ok(EpochOracle::Hybrid(oracle))
        },
    }
}

async fn create_base_layer_epoch_oracle<TStore: EpochOracleStore + 'static>(
    config: &ApplicationConfig,
    store: TStore,
    consensus_constants: &ConsensusConstants,
) -> Result<BaseLayerOracle<TStore>, ExitError> {
    let mut base_node_client = create_base_layer_client(config).await?;
    verify_correct_network(&mut base_node_client, config.network).await?;
    Ok(BaseLayerOracle::new(
        store,
        base_node_client,
        consensus_constants.base_layer_confirmations,
        config.epoch_oracle.base_layer.scanning_interval,
        config.indexer.sidechain_id.as_ref().map(|p| p.to_byte_type()),
        config
            .indexer
            .burnt_utxo_sidechain_id
            .as_ref()
            .map(|p| p.to_byte_type()),
        config.indexer.sidechain_id.as_ref().map(|p| p.to_byte_type()),
    ))
}

async fn create_configured_epoch_oracle<TStore: EpochOracleStore + Send>(
    config: &ApplicationConfig,
    store: TStore,
) -> Result<ConfiguredEpochOracle<TStore, IntervalEpochTicker>, ExitError> {
    let oracle_config = config.epoch_oracle.configured.load().await?;
    Ok(ConfiguredEpochOracle::new(oracle_config, store))
}

async fn create_hybrid_epoch_oracle<TStore: EpochOracleStore + Clone + Send + 'static>(
    config: &ApplicationConfig,
    store: TStore,
    consensus_constants: &ConsensusConstants,
) -> Result<HybridEpochOracle<TStore>, ExitError> {
    let base_layer_oracle = create_base_layer_epoch_oracle(config, store.clone(), consensus_constants).await?;
    let oracle_config = config.epoch_oracle.configured.load().await?;
    let (ticker, trigger) = watch_ticker();
    let configured_oracle = ConfiguredEpochOracle::with_custom_ticker(oracle_config, store, ticker);
    Ok(HybridEpochOracle::new(configured_oracle, base_layer_oracle, trigger))
}
