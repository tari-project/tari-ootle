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
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

use reqwest::Url;
use tari_common::configuration::CommonConfig;
use tari_ootle_common_types::Network;
use tari_ootle_walletd::{
    config::{ApplicationConfig, WalletDaemonAuth, WalletDaemonConfig},
    run_tari_ootle_walletd,
};
use tari_shutdown::Shutdown;
use tari_transaction::TransactionId;
use tari_wallet_daemon_client::{
    error::WalletDaemonClientError,
    types::{
        AuthLoginAcceptRequest,
        AuthLoginRequest,
        AuthLoginResponse,
        ClaimBurnProof,
        ClaimBurnRequest,
        ClaimBurnResponse,
        TransactionWaitResultRequest,
        TransactionWaitResultResponse,
    },
    ComponentAddressOrName,
    WalletDaemonClient,
};
use tokio::task;

use crate::{
    cucumber_log,
    helpers::{check_join_handle, get_os_assigned_ports, wait_listener_on_local_port},
    logging::get_base_dir_for_scenario,
    TariWorld,
};

#[derive(Debug)]
pub struct TariWalletDaemonProcess {
    pub name: String,
    pub json_rpc_port: u16,
    pub indexer_api_port: u16,
    pub temp_path_dir: PathBuf,
    pub shutdown: Shutdown,
}

pub async fn spawn_wallet_daemon(world: &mut TariWorld, wallet_daemon_name: String, indexer_name: String) {
    let (signaling_server_port, json_rpc_port) = get_os_assigned_ports();
    let base_dir = get_base_dir_for_scenario("wallet_daemon", world.get_current_scenario_name(), &wallet_daemon_name);

    let indexer_api_port = world.get_indexer(&indexer_name).api_port;
    let shutdown = Shutdown::new();
    let shutdown_signal = shutdown.to_signal();

    let json_rpc_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), json_rpc_port);
    let signaling_server_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), signaling_server_port);
    let indexer_url = format!("http://127.0.0.1:{}", indexer_api_port);

    let mut config = ApplicationConfig {
        common: CommonConfig::default(),
        ootle_wallet_daemon: WalletDaemonConfig::default(),
    };

    config.common.base_path.clone_from(&base_dir);
    config.ootle_wallet_daemon.json_rpc_address = Some(json_rpc_address);
    config.ootle_wallet_daemon.signaling_server_address = Some(signaling_server_addr);
    config.ootle_wallet_daemon.indexer_api_url = indexer_url.parse().unwrap();
    config.ootle_wallet_daemon.network = Network::LocalNet;
    config.ootle_wallet_daemon.authentication = WalletDaemonAuth::None;
    config.ootle_wallet_daemon.override_keyring_password = Some("secret".into());
    // Avoid using keyring in cucumber tests

    let handle = task::spawn(run_tari_ootle_walletd(config, None, shutdown_signal));

    // Wait for node to start up
    let handle = wait_listener_on_local_port(handle, json_rpc_port).await;
    // Check if the task errored/panicked
    let _handle = check_join_handle(&wallet_daemon_name, handle).await;

    let wallet_daemon_process = TariWalletDaemonProcess {
        name: wallet_daemon_name.clone(),
        json_rpc_port,
        indexer_api_port,
        temp_path_dir: base_dir,
        shutdown,
    };

    crate::cucumber_log!("Wallet daemon {} started", wallet_daemon_name);
    world.wallet_daemons.insert(wallet_daemon_name, wallet_daemon_process);
}

impl TariWalletDaemonProcess {
    pub fn stop(&mut self) {
        self.shutdown.trigger();
    }

    fn get_client(&self) -> WalletDaemonClient {
        let endpoint = Url::parse(&format!("http://127.0.0.1:{}", self.json_rpc_port)).unwrap();
        WalletDaemonClient::connect(endpoint, None).unwrap()
    }

    pub async fn get_authed_client(&self) -> WalletDaemonClient {
        cucumber_log!("Authenticating wallet daemon {}", self.name);
        let mut client = self.get_client();
        // authentication
        let AuthLoginResponse { auth_token, .. } = client
            .auth_request(AuthLoginRequest {
                permissions: vec!["Admin".to_string()],
                duration: None,
                webauthn_finish_auth_request: None,
            })
            .await
            .unwrap();
        let auth_response = client
            .auth_accept(AuthLoginAcceptRequest {
                auth_token,
                name: "Testing Token".to_string(),
            })
            .await
            .unwrap();
        cucumber_log!("Authenticated wallet daemon {}", self.name);
        client.set_auth_token(auth_response.permissions_token);
        client
    }

    pub async fn claim_burn(
        &self,
        account_name: &str,
        claim_proof: ClaimBurnProof,
    ) -> Result<ClaimBurnResponse, WalletDaemonClientError> {
        cucumber_log!("Claiming burn for account {}", account_name);
        let mut client = self.get_authed_client().await;

        let req = ClaimBurnRequest {
            account: ComponentAddressOrName::Name(account_name.into()),
            claim_proof,
            max_fee: Some(5000),
        };

        client
            .claim_burn(req)
            .await
            .inspect_err(|e| cucumber_log!("Claim burn failed: {}", e))
    }

    pub async fn wait_for_transaction_result(&self, tx_id: TransactionId) -> TransactionWaitResultResponse {
        let mut client = self.get_authed_client().await;
        client
            .wait_transaction_result(TransactionWaitResultRequest {
                transaction_id: tx_id,
                timeout_secs: Some(30),
            })
            .await
            .unwrap()
    }
}
