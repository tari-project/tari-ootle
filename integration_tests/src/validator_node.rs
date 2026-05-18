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

use std::{
    fs,
    fs::File,
    path::{Path, PathBuf},
};

use multiaddr::Multiaddr;
use ootle_byte_type::FromByteType;
use reqwest::Url;
use tari_common::configuration::{CommonConfig, StringList};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_ootle_address::Network;
use tari_ootle_app_utilities::{
    epoch_oracle_config::EpochOracleConfig,
    keypair::create_new_keypair,
    p2p_config::PeerSeedsConfig,
};
use tari_ootle_common_types::layer_one_transaction::{LayerOneTransactionDef, ValidatorRegistrationParams};
use tari_shutdown::Shutdown;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;
use tari_validator_node::{ApplicationConfig, ValidatorNodeConfig, run_validator_node};
use tari_validator_node_client::{
    ValidatorNodeClient,
    types::{LayerOneTransactionParams, PrepareLayerOneTransactionRequest},
};
use tokio::task;

use crate::{
    TariWorld,
    cucumber_log,
    helpers::{check_join_handle, get_os_assigned_port, get_os_assigned_ports, wait_listener_on_local_port},
    logging::get_base_dir_for_scenario,
};

#[derive(Debug)]
pub struct ValidatorNodeProcess {
    pub name: String,
    pub public_key: RistrettoPublicKeyBytes,
    pub p2p_port: u16,
    pub json_rpc_port: u16,
    pub web_ui_port: u16,
    pub base_node_grpc_port: u16,
    pub handle: task::JoinHandle<Result<(), anyhow::Error>>,
    pub temp_dir_path: PathBuf,
    pub shutdown: Shutdown,
}

impl ValidatorNodeProcess {
    pub fn create_client(&self) -> ValidatorNodeClient {
        get_vn_client(self.json_rpc_port)
    }

    pub fn get_addresses(&self) -> Vec<Multiaddr> {
        let addr = format!("/ip4/127.0.0.1/tcp/{}", self.p2p_port)
            .parse()
            .expect("Could not parse multiaddr");
        vec![addr]
    }

    pub fn layer_one_transaction_path(&self) -> PathBuf {
        self.temp_dir_path.join("data/layer_one_transactions")
    }

    pub async fn save_database(&self, database_name: String, to: &Path) {
        fs::create_dir_all(to).expect("Could not create directory");
        let from = &self.temp_dir_path.join(format!("{}.db", database_name));
        fs::copy(from, to.join(format!("{}.sqlite", database_name))).expect("Could not copy file");
    }

    pub async fn get_registration_info(&self) -> LayerOneTransactionDef<ValidatorRegistrationParams> {
        let mut client = self.create_client();
        let resp = client
            .prepare_layer_one_transaction(PrepareLayerOneTransactionRequest {
                params: LayerOneTransactionParams::Registration,
            })
            .await
            .expect("Could not prepare transaction");
        let file = File::open(resp.path).expect("Could not open file");
        serde_json::from_reader(file).expect("Could not parse file")
    }

    pub async fn wait_for_consensus_to_start(&self) {
        let mut client = self.create_client();
        let mut attempts = 60;
        loop {
            let resp = client.get_consensus_status().await.unwrap();
            if resp.state == "Running" {
                return;
            }
            attempts -= 1;
            if attempts == 0 {
                panic!(
                    "Validator node {} did not start consensus in time: status: {}, epoch: {}",
                    self.name, resp.state, resp.epoch
                );
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}

pub async fn spawn_validator_node(
    world: &mut TariWorld,
    validator_node_name: String,
    base_node_name: String,
    claim_account_name: Option<&str>,
) -> ValidatorNodeProcess {
    let base_node_grpc_port = world.base_nodes.get(&base_node_name).unwrap().grpc_port;
    let fee_claim_public_key = claim_account_name
        .map(|n| {
            let account = world.wallet_accounts.get(n).unwrap_or_else(|| {
                panic!("No account named {n} found in world.wallet_accounts");
            });
            account.address.account_public_key().try_from_byte_type().unwrap()
        })
        .unwrap_or_default();
    let temp_dir = get_base_dir_for_scenario(
        "validator_node",
        world.current_scenario_name.as_ref().unwrap(),
        &validator_node_name,
    );
    let peer_seeds = collect_peer_seeds(world);

    spawn_validator_node_process(ValidatorSpawn {
        name: validator_node_name,
        temp_dir,
        base_node_grpc_port,
        fee_claim_public_key,
        peer_seeds,
    })
    .await
}

/// Restart a previously-stopped validator node against its existing data directory.
/// Takes the existing `temp_dir` / `base_node_grpc_port` from the VN being replaced;
/// fresh p2p / json-rpc ports are OS-assigned. Keypair is loaded from the identity
/// file so the VN's public key and L1 registration stay stable.
///
/// Caller is responsible for re-wiring peers — this function does not touch
/// `world.validator_nodes` for any other VN.
pub async fn respawn_validator_node(world: &mut TariWorld, validator_node_name: String) -> ValidatorNodeProcess {
    let old = world
        .validator_nodes
        .shift_remove(&validator_node_name)
        .unwrap_or_else(|| panic!("Validator node {validator_node_name} not found for respawn"));
    let fee_claim_public_key = world
        .wallet_accounts
        .values()
        .next()
        .map(|a| a.address.account_public_key().try_from_byte_type().unwrap())
        .unwrap_or_default();
    let peer_seeds = collect_peer_seeds(world);

    spawn_validator_node_process(ValidatorSpawn {
        name: validator_node_name,
        temp_dir: old.temp_dir_path,
        base_node_grpc_port: old.base_node_grpc_port,
        fee_claim_public_key,
        peer_seeds,
    })
    .await
}

fn collect_peer_seeds(world: &TariWorld) -> Vec<String> {
    world
        .vn_seeds
        .values()
        .map(|vn| format!("{}::/ip4/127.0.0.1/tcp/{}", vn.public_key, vn.p2p_port))
        .collect()
}

struct ValidatorSpawn {
    name: String,
    temp_dir: PathBuf,
    base_node_grpc_port: u16,
    fee_claim_public_key: RistrettoPublicKey,
    peer_seeds: Vec<String>,
}

async fn spawn_validator_node_process(spawn: ValidatorSpawn) -> ValidatorNodeProcess {
    let ValidatorSpawn {
        name,
        temp_dir,
        base_node_grpc_port,
        fee_claim_public_key,
        peer_seeds,
    } = spawn;
    let (port, json_rpc_port) = get_os_assigned_ports();
    let web_ui_port = get_os_assigned_port();
    let shutdown = Shutdown::new();
    let handle = task::spawn({
        let shutdown = shutdown.clone();
        let temp_dir = temp_dir.clone();
        async move {
            let mut config = ApplicationConfig {
                common: CommonConfig::default(),
                validator_node: ValidatorNodeConfig::default(),
                epoch_oracle: EpochOracleConfig::default(),
                peer_seeds: PeerSeedsConfig::default(),
                network: Network::LocalNet,
            };
            cucumber_log!("Using validator_node temp_dir: {}", temp_dir.display());
            config.common.base_path.clone_from(&temp_dir);
            config.network = Network::LocalNet;
            config.validator_node.set_base_path(&temp_dir);
            config.validator_node.shard_key_file = temp_dir.join("shard_key.json");
            config.validator_node.identity_file = temp_dir.join("validator_node_id.json");
            config.epoch_oracle.base_layer.base_node_grpc_url =
                Some(format!("http://127.0.0.1:{}", base_node_grpc_port).parse().unwrap());
            config.validator_node.p2p.enable_mdns = false;
            config.validator_node.json_rpc_listener_address =
                Some(format!("127.0.0.1:{}", json_rpc_port).parse().unwrap());
            config.validator_node.web_ui_listener_address = Some(format!("127.0.0.1:{}", web_ui_port).parse().unwrap());
            config.validator_node.p2p.listener_port = port;
            config.validator_node.fee_claim_public_key = fee_claim_public_key;
            config.peer_seeds.peer_seeds = StringList::from(peer_seeds);

            // Load-if-exists: fresh dirs get a random identity + save; restarts reuse
            // the on-disk keypair so the VN's public key / L1 registration is stable.
            let keypair = load_or_create_keypair(&config.validator_node.identity_file);
            run_validator_node(keypair, config, shutdown).await
        }
    });

    let handle = wait_listener_on_local_port(handle, json_rpc_port).await;
    let handle = check_join_handle(&name, handle).await;
    let public_key = get_vn_identity(json_rpc_port).await;

    crate::cucumber_log!("Validator node {} started", name);
    ValidatorNodeProcess {
        name,
        public_key,
        p2p_port: port,
        base_node_grpc_port,
        web_ui_port,
        handle,
        json_rpc_port,
        temp_dir_path: temp_dir,
        shutdown,
    }
}

fn load_or_create_keypair(path: &Path) -> tari_ootle_app_utilities::keypair::RistrettoKeypair {
    if path.exists() {
        tari_ootle_app_utilities::keypair::setup_keypair_prompt(path, true).expect("load existing keypair")
    } else {
        create_new_keypair(path).expect("create new keypair")
    }
}

fn get_vn_client(port: u16) -> ValidatorNodeClient {
    let endpoint: Url = Url::parse(&format!("http://localhost:{}", port)).unwrap();
    ValidatorNodeClient::connect(endpoint).unwrap()
}

async fn get_vn_identity(jrpc_port: u16) -> RistrettoPublicKeyBytes {
    // send the JSON RPC "get_identity" request to the VN
    let mut client = get_vn_client(jrpc_port);
    let resp = client.get_identity().await.unwrap();
    resp.public_key
}

impl ValidatorNodeProcess {
    pub fn stop(&mut self) {
        self.shutdown.trigger();
    }

    /// Trigger shutdown and await the backing task so the RocksDB lock is released
    /// before the caller touches the data directory (e.g. the offline rollback tool).
    pub async fn stop_and_wait(&mut self) {
        self.shutdown.trigger();
        // Task handle must be awaited to ensure the VN's Drop path ran and rocksdb
        // closed. Replace with a completed handle so this is idempotent.
        let handle = std::mem::replace(&mut self.handle, task::spawn(async { Ok(()) }));
        match handle.await {
            Ok(Ok(_)) => {},
            Ok(Err(err)) => {
                crate::cucumber_log!("Validator node {} exited with error: {err}", self.name);
            },
            Err(err) => {
                crate::cucumber_log!("Validator node {} task join error: {err}", self.name);
            },
        }
    }

    /// Absolute path to the validator's on-disk RocksDB state store. Mirrors the
    /// default resolved by `ValidatorNodeConfig::set_base_path` in
    /// `spawn_validator_node`.
    pub fn state_db_path(&self) -> PathBuf {
        self.temp_dir_path.join("data").join("validator_node").join("rocksdb")
    }

    pub fn get_client(&self) -> ValidatorNodeClient {
        let endpoint: Url = Url::parse(&format!("http://localhost:{}", self.json_rpc_port)).unwrap();
        ValidatorNodeClient::connect(endpoint).unwrap()
    }
}
