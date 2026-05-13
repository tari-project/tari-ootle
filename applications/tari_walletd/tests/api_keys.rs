//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    time::Duration,
};

use tari_common::configuration::CommonConfig;
use tari_ootle_address::Network;
use tari_ootle_wallet_sdk::models::KeyBranch;
use tari_ootle_walletd::{
    config::{ApplicationConfig, WalletDaemonAuth, WalletDaemonConfig},
    run_tari_ootle_walletd,
};
use tari_ootle_walletd_client::{
    WalletDaemonClient,
    permissions::JrpcPermission,
    types::{
        AuthCreateApiKeyRequest, AuthCredentials, AuthListApiKeysRequest, AuthLoginRequest, AuthRevokeApiKeyRequest,
    },
};
use tari_shutdown::Shutdown;
use tempfile::TempDir;
use tokio::{net::TcpStream, task::JoinHandle, time};
use url::Url;

struct TestWalletDaemon {
    endpoint: Url,
    shutdown: Shutdown,
    handle: JoinHandle<Result<(), anyhow::Error>>,
    _temp_dir: TempDir,
}

impl TestWalletDaemon {
    async fn spawn() -> Self {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().join("data");
        let burn_proof_dir = temp_dir.path().join("burn_proofs");
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&burn_proof_dir).unwrap();

        let json_rpc_port = get_os_assigned_port();
        let signaling_server_port = get_os_assigned_port();
        let json_rpc_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), json_rpc_port);
        let signaling_server_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), signaling_server_port);

        let mut config = ApplicationConfig {
            common: CommonConfig::default(),
            ootle_wallet_daemon: WalletDaemonConfig::default(),
        };
        config.common.base_path = temp_dir.path().to_path_buf();
        config.ootle_wallet_daemon.json_rpc_address = json_rpc_address;
        config.ootle_wallet_daemon.signaling_server_address = Some(signaling_server_address);
        config.ootle_wallet_daemon.indexer_api_url = "http://127.0.0.1:1".parse().unwrap();
        config.ootle_wallet_daemon.network = Network::LocalNet;
        config.ootle_wallet_daemon.authentication = WalletDaemonAuth::None;
        config.ootle_wallet_daemon.override_keyring_password = Some("secret".into());
        config.ootle_wallet_daemon.burn_proof_dir = Some(burn_proof_dir);
        config.ootle_wallet_daemon.auto_claim_burns = false;

        let shutdown = Shutdown::new();
        let handle = tokio::spawn(run_tari_ootle_walletd(config, None, shutdown.to_signal()));
        wait_for_listener(json_rpc_address).await;

        Self {
            endpoint: Url::parse(&format!("http://127.0.0.1:{json_rpc_port}/json_rpc")).unwrap(),
            shutdown,
            handle,
            _temp_dir: temp_dir,
        }
    }

    async fn stop(mut self) {
        self.shutdown.trigger();
        let _ = time::timeout(Duration::from_secs(5), self.handle).await;
    }
}

fn get_os_assigned_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn wait_for_listener(addr: SocketAddr) {
    for _ in 0..100 {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        time::sleep(Duration::from_millis(50)).await;
    }
    panic!("wallet daemon did not listen on {addr}");
}

async fn admin_client(endpoint: Url) -> WalletDaemonClient {
    let mut client = WalletDaemonClient::connect(endpoint, None).unwrap();
    let login = client
        .auth_request(AuthLoginRequest {
            permissions: vec![JrpcPermission::Admin],
            credentials: AuthCredentials::None,
        })
        .await
        .unwrap();
    client.set_auth_token(login.token);
    client
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_key_auth_flow_enforces_scope_and_revocation() {
    let daemon = TestWalletDaemon::spawn().await;
    let mut admin = admin_client(daemon.endpoint.clone()).await;

    let created = admin
        .auth_create_api_key(AuthCreateApiKeyRequest {
            name: "agent-key-list".to_string(),
            permissions: vec![JrpcPermission::KeyList],
            confirm_admin: false,
        })
        .await
        .unwrap();

    assert!(created.api_key.starts_with("twda_"));
    assert_eq!(created.key.name, "agent-key-list");
    assert_eq!(created.key.permissions, vec![JrpcPermission::KeyList]);

    let listed = admin.auth_list_api_keys(AuthListApiKeysRequest {}).await.unwrap();
    assert_eq!(listed.api_keys.len(), 1);
    assert_eq!(listed.api_keys[0].id, created.key.id);

    let mut non_admin = WalletDaemonClient::connect(daemon.endpoint.clone(), None).unwrap();
    let login = non_admin
        .auth_request(AuthLoginRequest {
            permissions: vec![JrpcPermission::KeyList],
            credentials: AuthCredentials::None,
        })
        .await
        .unwrap();
    non_admin.set_auth_token(login.token);
    let non_admin_create = non_admin
        .auth_create_api_key(AuthCreateApiKeyRequest {
            name: "should-fail".to_string(),
            permissions: vec![JrpcPermission::KeyList],
            confirm_admin: false,
        })
        .await;
    assert!(non_admin_create.is_err());

    let mut agent = WalletDaemonClient::connect_with_api_key(daemon.endpoint.clone(), created.api_key.as_str())
        .await
        .unwrap();
    agent.list_keys(KeyBranch::Account).await.unwrap();
    let out_of_scope = agent.create_key(KeyBranch::Account).await;
    assert!(out_of_scope.is_err());

    admin
        .auth_revoke_api_key(AuthRevokeApiKeyRequest { id: created.key.id })
        .await
        .unwrap();
    let listed = admin.auth_list_api_keys(AuthListApiKeysRequest {}).await.unwrap();
    assert!(listed.api_keys.is_empty());

    let revoked_jwt_use = agent.list_keys(KeyBranch::Account).await;
    assert!(revoked_jwt_use.is_err());

    let revoked_raw_key_login =
        WalletDaemonClient::connect_with_api_key(daemon.endpoint.clone(), created.api_key.as_str()).await;
    assert!(revoked_raw_key_login.is_err());

    daemon.stop().await;
}
