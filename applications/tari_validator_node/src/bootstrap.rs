//   Copyright 2022. The Tari Project
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

use std::{collections::HashMap, fs, io, str::FromStr, sync::Arc};

use anyhow::{anyhow, Context};
use futures::{future, FutureExt};
use libp2p::identity;
use log::*;
use minotari_app_utilities::identity_management;
use tari_base_node_client::grpc::GrpcBaseNodeClient;
use tari_common::{
    configuration::bootstrap::{grpc_default_port, ApplicationType},
    exit_codes::{ExitCode, ExitError},
};
use tari_consensus::consensus_constants::ConsensusConstants;
#[cfg(not(feature = "metrics"))]
use tari_consensus::traits::hooks::NoopHooks;
use tari_crypto::tari_utilities::ByteArray;
use tari_engine_types::ToByteType;
use tari_epoch_manager::{
    service::{EpochManagerConfig, EpochManagerHandle},
    EpochManagerReader,
};
use tari_epoch_oracles::{
    base_layer::{
        BaseLayerBlockHeaderStore,
        BaseLayerEpochOracleConfig,
        BaseLayerEpochOracleFeatures,
        BaseLayerOracle,
    },
    configured::{ConfiguredEpochOracle, RealTimeEpochTicker},
    hybrid::{watch_ticker, HybridEpochOracle},
    store::EpochOracleStore,
    EpochOracle,
};
use tari_networking::{MessagingMode, NetworkingHandle, RelayCircuitLimits, RelayReservationLimits, SwarmConfig};
use tari_ootle_app_utilities::{
    claim_burn_proof_verifier::TariClaimBurnProofVerifier,
    common::verify_correct_network,
    configuration::convert_network_to_l1_network,
    epoch_oracle_config::{BaseLayerOracleConfig, EpochOracleType},
    fee_tables::get_fee_table_by_network,
    keypair::RistrettoKeypair,
    seed_peer::SeedPeer,
    transaction_executor::TariTransactionProcessor,
};
use tari_ootle_common_types::{services::template_provider::TemplateProvider, Network, PeerAddress};
use tari_ootle_p2p::TariMessagingSpec;
use tari_ootle_storage::{global::GlobalDb, StateStore};
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_rpc_framework::RpcServer;
use tari_shutdown::ShutdownSignal;
use tari_state_store_rocksdb::DatabaseOptions;
use tari_transaction::Transaction;
use tari_validator_node_rpc::client::TariValidatorNodeRpcClientFactory;
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
};

#[cfg(feature = "metrics")]
use crate::consensus::metrics::PrometheusConsensusMetrics;
use crate::{
    consensus::{self, ConsensusHandle, TarBlockTransactionExecutor, ValidationContext},
    file_l1_submitter::FileLayerOneSubmitter,
    genesis_state::create_genesis_state,
    p2p::{
        create_tari_validator_node_rpc_service,
        services::{
            consensus_gossip::{self},
            mempool::{self, MempoolHandle},
            messaging::{ConsensusInboundMessaging, ConsensusOutboundMessaging},
        },
        NopLogger,
    },
    state_store_template_provider::StateStoreTemplateProvider,
    transaction_validators::{
        BasicValidations,
        EpochRangeValidator,
        TemplateExistsValidator,
        TransactionDryRunValidator,
        TransactionNetworkValidator,
        TransactionSignatureValidator,
        TransactionValidationError,
        WithContext,
    },
    validator::Validator,
    ApplicationConfig,
    ValidatorNodeEpochManagerSpec,
    ValidatorNodeStateStore,
};

const LOG_TARGET: &str = "tari::validator_node::bootstrap";

#[allow(clippy::too_many_lines)]
pub async fn spawn_services(
    config: ApplicationConfig,
    shutdown: ShutdownSignal,
    keypair: RistrettoKeypair,
    global_db: GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>,
    consensus_constants: ConsensusConstants,
    #[cfg(feature = "metrics")] metrics_registry: &mut prometheus_client::registry::Registry,
) -> Result<Services<ValidatorNodeStateStore>, anyhow::Error> {
    let mut handles = Vec::with_capacity(10);

    ensure_directories_exist(&config)?;

    // Networking
    let (tx_consensus_messages, rx_consensus_messages) = mpsc::unbounded_channel();

    // gossip channels
    let (tx_transaction_gossip_messages, rx_transaction_gossip_messages) = mpsc::unbounded_channel();
    let (tx_consensus_gossip_messages, rx_consensus_gossip_messages) = mpsc::unbounded_channel();
    let mut tx_gossip_messages_by_topic = HashMap::new();
    tx_gossip_messages_by_topic.insert(mempool::TOPIC_PREFIX.to_string(), tx_transaction_gossip_messages);
    tx_gossip_messages_by_topic.insert(consensus_gossip::TOPIC_PREFIX.to_string(), tx_consensus_gossip_messages);

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
        .collect::<anyhow::Result<Vec<_>>>()?;
    let seed_peers = seed_peers
        .into_iter()
        .map(|p| {
            let peer_id = p.to_peer_id();
            (peer_id, p.into_address())
        })
        .collect();
    #[allow(unused_mut)]
    let mut network_builder = tari_networking::Builder::<TariMessagingSpec>::new(identity)
        .with_messaging_mode(MessagingMode::Enabled {
            tx_messages: tx_consensus_messages,
            tx_gossip_messages_by_topic,
        })
        .with_config(tari_networking::Config {
            // TODO: configurable
            listeners: vec![
                format!("/ip4/0.0.0.0/tcp/{}", config.validator_node.p2p.listener_port)
                    .parse()
                    .expect("Failed to parse listener address"),
                format!("/ip4/0.0.0.0/udp/{}/quic-v1", config.validator_node.p2p.listener_port)
                    .parse()
                    .expect("Failed to parse listener address"),
            ],
            swarm: SwarmConfig {
                protocol_version: format!("/tari/{}/0.0.1", config.network).parse().unwrap(),
                user_agent: "/tari/validator/0.0.1".to_string(),
                enable_mdns: config.validator_node.p2p.enable_mdns,
                enable_relay: true,
                // TODO: allow node operator to configure
                relay_circuit_limits: RelayCircuitLimits::high(),
                relay_reservation_limits: RelayReservationLimits::high(),
                rendezvous_server_enabled: config.validator_node.p2p.enable_rendezvous,
                ..Default::default()
            },
            reachability_mode: config.validator_node.p2p.reachability_mode.into(),
            announce: true,
            rendezvous_namespace: format!(
                "tari-{}",
                config.network.to_string().to_lowercase()
            ),
            ..Default::default()
        })
        .with_seed_peers(seed_peers)
        .then(|builder| {
            match config.peer_seeds.rendezvous_server.as_ref() {
                Some(server) => {
                    let server = SeedPeer::from_str(server)
                        .expect("Failed to parse rendezvous server seed peer");
                    if let Some(peer_id) = server.to_peer_id() {
                        let addr = server.address().clone();
                        builder.with_rendezvous_server(peer_id, addr)
                    } else {
                        warn!(target: LOG_TARGET, "Rendezvous server peer ID is not set, skipping rendezvous server configuration");
                        builder
                    }
                }
                None => builder,
            }
        });

    #[cfg(feature = "metrics")]
    {
        network_builder = network_builder.with_metrics(metrics_registry);
    }
    let (mut networking, join_handle) = network_builder.spawn(shutdown.clone())?;
    handles.push(join_handle);

    info!(target: LOG_TARGET, "Message logging initializing");

    info!(target: LOG_TARGET, "State store initializing");

    let state_store = ValidatorNodeStateStore::open(
        &config.validator_node.state_db_path,
        // TODO: just enable it always for now, later make it configurable and default to true for testnets
        DatabaseOptions::default().with_debugging_data(true),
    )?;

    state_store.with_write_tx(|tx| create_genesis_state(tx, config.network, consensus_constants.num_preshards))?;

    info!(target: LOG_TARGET, "Epoch manager initializing");
    let epoch_manager_config = EpochManagerConfig {
        base_layer_confirmations: consensus_constants.base_layer_confirmations,
        committee_size: consensus_constants
            .committee_size
            .try_into()
            .context("committee size must be non-zero")?,
        validator_node_sidechain_id: config.validator_node.validator_node_sidechain_id.to_byte_type(),
        fee_claim_public_key: config.validator_node.fee_claim_public_key.to_byte_type(),
        num_preshards: consensus_constants.num_preshards,
    };

    // Epoch event scanner
    let epoch_event_oracle = create_epoch_oracle(&config, global_db.clone(), &consensus_constants).await?;

    let layer_one_transaction_submitter = FileLayerOneSubmitter::new(config.get_layer_one_transaction_base_path());

    // Epoch manager
    let (epoch_manager, epoch_manager_join_handle) =
        tari_epoch_manager::service::spawn_service::<ValidatorNodeEpochManagerSpec>(
            epoch_manager_config,
            global_db.clone(),
            keypair.public_key().to_byte_type(),
            epoch_event_oracle,
            layer_one_transaction_submitter.clone(),
            shutdown.clone(),
        );

    handles.push(epoch_manager_join_handle);

    let validator_node_client_factory = TariValidatorNodeRpcClientFactory::new(networking.clone());

    info!(target: LOG_TARGET, "Template manager initializing");
    // Template manager
    let template_provider = StateStoreTemplateProvider::new(state_store.clone(), &config.validator_node.templates);

    info!(target: LOG_TARGET, "Payload processor initializing");
    // Payload processor

    let (tx_hotstuff_events, _) = broadcast::channel(100);
    // Consensus gossip
    let (consensus_gossip_service, join_handle, rx_consensus_gossip_messages) = consensus_gossip::spawn(
        epoch_manager.subscribe(),
        tx_hotstuff_events.subscribe(),
        networking.clone(),
        rx_consensus_gossip_messages,
    );
    handles.push(join_handle);

    // Messaging
    let message_logger = NopLogger; // SqliteMessageLogger::new(config.validator_node.data_dir.join("message_log.sqlite"));
    let local_address = PeerAddress::from(keypair.public_key().clone());
    let (loopback_sender, loopback_receiver) = mpsc::unbounded_channel();
    let inbound_messaging = ConsensusInboundMessaging::new(
        local_address,
        rx_consensus_messages,
        rx_consensus_gossip_messages,
        loopback_receiver,
        message_logger.clone(),
    );
    let outbound_messaging = ConsensusOutboundMessaging::new(
        loopback_sender,
        consensus_gossip_service.clone(),
        networking.clone(),
        message_logger.clone(),
    );

    // Transaction executor
    let fee_table = get_fee_table_by_network(config.network);
    let transaction_processor = TariTransactionProcessor::new(
        template_provider.clone(),
        fee_table.clone(),
        Arc::new(TariClaimBurnProofVerifier::new(config.network, global_db.clone())),
    );
    let transaction_executor = TarBlockTransactionExecutor::new(
        transaction_processor,
        create_consensus_transaction_validator(config.network, template_provider.clone()).boxed(),
    );

    #[cfg(feature = "metrics")]
    let tari_metrics_registry = metrics_registry.sub_registry_with_prefix("tari");
    #[cfg(feature = "metrics")]
    let metrics = PrometheusConsensusMetrics::register(tari_metrics_registry);
    #[cfg(not(feature = "metrics"))]
    let metrics = NoopHooks;

    let sidechain_id = config
        .validator_node
        .validator_node_sidechain_id
        .as_ref()
        .map(|pk| pk.to_byte_type());

    // Consensus
    let signing_service = consensus::TariSignatureService::new(keypair.clone());
    let (consensus_join_handle, consensus_handle) = consensus::spawn(
        config.network,
        &config.validator_node.consensus,
        sidechain_id,
        state_store.clone(),
        local_address,
        signing_service,
        epoch_manager.clone(),
        inbound_messaging,
        outbound_messaging.clone(),
        validator_node_client_factory.clone(),
        metrics,
        shutdown.clone(),
        transaction_executor,
        tx_hotstuff_events,
        consensus_constants.clone(),
    )
    .await;
    handles.push(consensus_join_handle);

    let (mempool, join_handle) = mempool::spawn(
        epoch_manager.clone(),
        create_mempool_transaction_validator(config.network, template_provider.clone()),
        state_store.clone(),
        consensus_handle.clone(),
        networking.clone(),
        rx_transaction_gossip_messages,
        #[cfg(feature = "metrics")]
        tari_metrics_registry,
    );
    handles.push(join_handle);

    spawn_p2p_rpc(
        &config,
        &mut networking,
        epoch_manager.clone(),
        state_store.clone(),
        mempool.clone(),
        consensus_handle.clone(),
    )
    .await?;
    // Save final node identity after comms has initialized. This is required because the public_address can be
    // changed by comms during initialization when using tor.
    save_identities(&config, &keypair)?;

    Ok(Services {
        config,
        keypair,
        networking,
        mempool,
        epoch_manager,
        global_db,
        template_provider,
        consensus_handle,
        state_store,
        handles,
        layer_one_transaction_submitter,
    })
}

fn save_identities(config: &ApplicationConfig, keypair: &RistrettoKeypair) -> Result<(), ExitError> {
    identity_management::save_as_json(&config.validator_node.identity_file, keypair)
        .map_err(|e| ExitError::new(ExitCode::ConfigError, format!("Failed to save node identity: {}", e)))?;

    Ok(())
}

fn ensure_directories_exist(config: &ApplicationConfig) -> io::Result<()> {
    fs::create_dir_all(&config.validator_node.data_dir)?;
    Ok(())
}
pub struct Services<TStore> {
    pub config: ApplicationConfig,
    pub keypair: RistrettoKeypair,
    pub networking: NetworkingHandle<TariMessagingSpec>,
    pub mempool: MempoolHandle,
    pub epoch_manager: EpochManagerHandle<PeerAddress>,
    pub template_provider: StateStoreTemplateProvider<ValidatorNodeStateStore>,
    pub consensus_handle: ConsensusHandle,
    pub state_store: TStore,
    pub global_db: GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>,
    pub layer_one_transaction_submitter: FileLayerOneSubmitter,

    pub handles: Vec<JoinHandle<Result<(), anyhow::Error>>>,
}

impl<TStore> Services<TStore> {
    pub async fn on_any_exit(&mut self) -> Result<(), anyhow::Error> {
        // JoinHandler panics if polled again after reading the Result, we fuse the future to prevent this.
        let fused = self.handles.iter_mut().map(|h| h.fuse());
        let (res, _, _) = future::select_all(fused).await;
        res.unwrap_or_else(|e| Err(anyhow!("Task panicked: {}", e)))
    }

    pub async fn join_all(self) -> Result<(), anyhow::Error> {
        let results = future::try_join_all(self.handles).await?;
        for res in results {
            res?;
        }
        Ok(())
    }
}

async fn spawn_p2p_rpc<TStateStore: StateStore + Clone + Send + Sync + 'static>(
    config: &ApplicationConfig,
    networking: &mut NetworkingHandle<TariMessagingSpec>,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    shard_store_store: TStateStore,
    mempool: MempoolHandle,
    consensus: ConsensusHandle,
) -> anyhow::Result<()> {
    let rpc_server = RpcServer::builder()
        .with_maximum_simultaneous_sessions(config.validator_node.rpc.max_simultaneous_sessions)
        .with_maximum_sessions_per_client(config.validator_node.rpc.max_sessions_per_client)
        .finish()
        .add_service(create_tari_validator_node_rpc_service(
            epoch_manager,
            shard_store_store,
            mempool,
            consensus,
        ));

    let (notify_tx, notify_rx) = mpsc::unbounded_channel();
    networking
        .add_protocol_notifier(rpc_server.all_protocols().iter().cloned(), notify_tx)
        .await?;
    tokio::spawn(rpc_server.serve(notify_rx));
    Ok(())
}

pub fn create_mempool_transaction_validator<TProvider: TemplateProvider>(
    network: Network,
    template_manager: TProvider,
) -> impl Validator<Transaction, Context = (), Error = TransactionValidationError> {
    TransactionNetworkValidator::new(network)
        .and_then(TransactionDryRunValidator)
        .and_then(BasicValidations::new())
        .and_then(TransactionSignatureValidator)
        .and_then(TemplateExistsValidator::new(template_manager))
}

pub fn create_consensus_transaction_validator<TProvider: TemplateProvider>(
    network: Network,
    template_manager: TProvider,
) -> impl Validator<Transaction, Context = ValidationContext, Error = TransactionValidationError> {
    WithContext::<ValidationContext, _, _>::new()
        .map_context(|_| (), create_mempool_transaction_validator(network, template_manager))
        .map_context(|c| c.current_epoch, EpochRangeValidator::new())
}

async fn create_base_layer_client(
    network: Network,
    config: &BaseLayerOracleConfig,
) -> Result<GrpcBaseNodeClient, ExitError> {
    let base_node_address = config.base_node_grpc_url.clone().unwrap_or_else(|| {
        let port = grpc_default_port(ApplicationType::BaseNode, convert_network_to_l1_network(&network));
        format!("http://127.0.0.1:{port}")
            .parse()
            .expect("Default base node GRPC URL is malformed")
    });
    info!(target: LOG_TARGET, "Connecting to base node on GRPC at {}", base_node_address);
    let base_node_client = GrpcBaseNodeClient::connect(base_node_address.clone())
        .await
        .map_err(|error| {
            ExitError::new(
                ExitCode::ConfigError,
                format!(
                    "Could not connect to the Minotari node at address {base_node_address}: {error}. Please ensure \
                     that the Minotari node is running and configured for GRPC."
                ),
            )
        })?;

    Ok(base_node_client)
}

async fn create_epoch_oracle<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + Send + Clone + 'static>(
    config: &ApplicationConfig,
    store: TStore,
    consensus_constants: &ConsensusConstants,
) -> anyhow::Result<EpochOracle<TStore>> {
    match config.epoch_oracle.oracle_type {
        EpochOracleType::BaseLayer => {
            let features = BaseLayerEpochOracleFeatures {
                sync_headers: true,
                sync_validator_node_changes: true,
            };
            let oracle = create_base_layer_epoch_oracle(config, store, consensus_constants, features).await?;
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

async fn create_base_layer_epoch_oracle<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + 'static>(
    config: &ApplicationConfig,
    store: TStore,
    consensus_constants: &ConsensusConstants,
    features: BaseLayerEpochOracleFeatures,
) -> anyhow::Result<BaseLayerOracle<TStore>> {
    let mut base_node_client = create_base_layer_client(config.network, &config.epoch_oracle.base_layer).await?;
    verify_correct_network(&mut base_node_client, config.network).await?;
    Ok(BaseLayerOracle::new(
        store,
        base_node_client,
        BaseLayerEpochOracleConfig {
            start_height: 0,
            height_lag: consensus_constants.base_layer_confirmations,
            scanning_interval: config.epoch_oracle.base_layer.scanning_interval,
            sidechain_id: config
                .validator_node
                .validator_node_sidechain_id
                .as_ref()
                .map(|p| p.to_byte_type()),
            features,
        },
        config.network,
    ))
}

async fn create_configured_epoch_oracle<TStore: EpochOracleStore + Send>(
    config: &ApplicationConfig,
    store: TStore,
) -> anyhow::Result<ConfiguredEpochOracle<TStore, RealTimeEpochTicker>> {
    let oracle_config = config.epoch_oracle.configured.load().await?;
    let oracle = ConfiguredEpochOracle::create(oracle_config, store)?;
    Ok(oracle)
}

async fn create_hybrid_epoch_oracle<TStore: EpochOracleStore + BaseLayerBlockHeaderStore + Clone + Send + 'static>(
    config: &ApplicationConfig,
    store: TStore,
    consensus_constants: &ConsensusConstants,
) -> anyhow::Result<HybridEpochOracle<TStore>> {
    let features = BaseLayerEpochOracleFeatures {
        sync_headers: true,
        // Dont sync validator node changes as they are handled by the configured oracle
        sync_validator_node_changes: false,
    };
    let base_layer_oracle =
        create_base_layer_epoch_oracle(config, store.clone(), consensus_constants, features).await?;
    let oracle_config = config.epoch_oracle.configured.load().await?;
    let (ticker, trigger) = watch_ticker();
    let configured_oracle = ConfiguredEpochOracle::with_custom_ticker(oracle_config, store, ticker);
    Ok(HybridEpochOracle::new(configured_oracle, base_layer_oracle, trigger))
}
