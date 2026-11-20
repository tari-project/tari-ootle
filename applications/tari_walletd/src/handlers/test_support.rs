//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A walletd handler context for handler tests, with helpers that mint each
//! kind of bearer a caller can present, and an indexer that serves signed
//! end-of-epoch checkpoints to a verifying wallet.

use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};

use axum::{
    Json,
    Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
use axum_extra::headers::{Authorization, authorization::Bearer};
use tari_indexer_client::types::{
    EpochCheckpointEntry,
    GetCommitteeRequest,
    GetCommitteeResponse,
    GetEpochManagerStatsResponse,
    GetLatestEpochCheckpointResponse,
    GetLatestShardGroupCheckpointsResponse,
    ValidatorSetCommittee,
};
use tari_ootle_address::Network;
use tari_ootle_common_types::{Epoch, ShardGroup};
use tari_ootle_light_client::test_fixtures::{
    CheckpointSpec,
    Signers,
    ValidatorSetFixture,
    checkpoint_entry,
    high,
    low,
};
use tari_ootle_wallet_sdk::{
    IndexerTrust,
    WalletSdkConfig,
    cipher_seed::CipherSeedRestore,
    models::EpochBirthday,
    storage::{WalletStoreWriter, WriteableWalletStore},
};
use tari_ootle_wallet_sdk_services::{
    account_monitor::AccountMonitor,
    indexer_rest_api::IndexerRestApiNetworkInterface,
    notify::Notify,
    transaction_service::TransactionService,
    utxo_scanner::StealthUtxoScannerWorker,
};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_ootle_walletd_client::permissions::Permissions;
use tari_shutdown::Shutdown;
use tari_utilities::SafePassword;

use crate::{
    WalletSdk,
    config::{WalletDaemonAuth, WalletDaemonConfig},
    handlers::{
        HandlerContext,
        auth::{api_keys, cookie, create_authenticator},
    },
};

/// An indexer URL on a closed port.
const CLOSED_INDEXER_URL: &str = "http://127.0.0.1:1";

pub(crate) struct TestDaemon {
    pub context: HandlerContext,
    /// The wallet's own interactive session, holding `admin`.
    pub session: Bearer,
    /// The daemon's local `.cookie` bearer token (`tc_...`).
    pub cookie: Bearer,
    _shutdown: Shutdown,
    _temp: tempfile::TempDir,
}

impl TestDaemon {
    /// A handler context on LocalNet whose indexer is a closed port, so any
    /// handler path that reaches the network fails fast.
    pub async fn start() -> Self {
        Self::init(false, CLOSED_INDEXER_URL, IndexerTrust::default()).await
    }

    /// Like [`Self::start`], with the transaction service left running so
    /// handlers can submit transactions.
    pub async fn start_with_transaction_service() -> Self {
        Self::init(true, CLOSED_INDEXER_URL, IndexerTrust::default()).await
    }

    /// A handler context on LocalNet that reads from `indexer` with `indexer_trust`.
    pub async fn start_with_indexer(indexer: &FakeIndexer, indexer_trust: IndexerTrust) -> Self {
        Self::init(false, indexer.url.as_str(), indexer_trust).await
    }

    async fn init(run_transaction_service: bool, indexer_url: &str, indexer_trust: IndexerTrust) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = SqliteWalletStore::try_open(temp.path().join("wallet.sqlite")).unwrap();
        store.run_migrations().unwrap();
        let mut sdk = WalletSdk::initialize_with_local_key_store(
            store.clone(),
            IndexerRestApiNetworkInterface::new(indexer_url),
            WalletSdkConfig {
                network: Network::LocalNet,
                override_keyring_password: Some(SafePassword::from_str("test wallet password").unwrap()),
                indexer_trust,
            },
            EpochBirthday::far_future(),
        )
        .unwrap();
        sdk.initialize_cipher_seed(CipherSeedRestore::CreateNewIfRequired)
            .unwrap();

        let notify = Notify::new(10);
        let shutdown = Shutdown::new();
        let (transaction_service, transaction_service_handle) =
            TransactionService::new(notify.clone(), sdk.clone(), shutdown.to_signal());
        let (utxo_worker, utxo_scanner_handle) = StealthUtxoScannerWorker::new(sdk.clone(), notify.clone()).spawn();
        let (account_monitor, account_monitor_handle) =
            AccountMonitor::new(notify.clone(), sdk.clone(), utxo_scanner_handle, shutdown.to_signal());
        let mut config = WalletDaemonConfig::default();
        config.network = Network::LocalNet;
        config.authentication = WalletDaemonAuth::None;
        let cookie_token = cookie::mint_cookie_token();
        let cookie_permissions = config.parsed_cookie_scopes().unwrap();
        let cookie_str = std::str::from_utf8(cookie_token.reveal()).expect("ASCII cookie token");
        let cookie = Authorization::<Bearer>::bearer(cookie_str).unwrap().0;
        let context = HandlerContext::new(
            sdk,
            notify,
            transaction_service_handle,
            account_monitor_handle,
            config.clone(),
            create_authenticator(&config, store).unwrap(),
            SafePassword::from_str("test jwt secret").unwrap(),
            shutdown.to_signal(),
        )
        .with_cookie_token(Some((cookie_token, cookie_permissions)));

        // The other background workers are not needed by handlers, so they stop now.
        drop(account_monitor);
        utxo_worker.abort();
        drop(utxo_worker.await);
        if run_transaction_service {
            tokio::spawn(transaction_service.run());
        } else {
            shutdown.trigger();
            drop(transaction_service);
        }

        let session = Self::jwt(&context, "admin", false);
        Self {
            context,
            session,
            cookie,
            _shutdown: shutdown,
            _temp: temp,
        }
    }

    /// Stores an API key named `name` holding `permissions`, bypassing the
    /// grant checks of `auth.create_api_key`, and returns its bearer.
    pub fn api_key(&self, name: &str, permissions: &str) -> Bearer {
        let raw = format!("{}{name}", api_keys::API_KEY_PREFIX);
        self.context
            .wallet_sdk()
            .store()
            .with_write_tx(|tx| tx.api_key_insert(name, &api_keys::hash_api_key(&raw), permissions, None))
            .unwrap();
        Authorization::<Bearer>::bearer(&raw).unwrap().0
    }

    /// A token like the one `webrtc.start` mints for a connected app.
    pub fn delegated_session(&self, permissions: &str) -> Bearer {
        Self::jwt(&self.context, permissions, true)
    }

    fn jwt(context: &HandlerContext, permissions: &str, delegated: bool) -> Bearer {
        let mut claims = context
            .jwt_api()
            .generate_auth_claims(Permissions::from_str(permissions).unwrap())
            .unwrap();
        claims.delegated = delegated;
        claims.delegation_id = delegated.then(rand::random);
        Authorization::<Bearer>::bearer(&context.jwt_api().grant(&claims).unwrap())
            .unwrap()
            .0
    }
}

/// The checkpoint of `shard_group` ending `current`'s epoch that hands over to `next`, signed by `signers`.
pub fn checkpoint_signed_by(
    shard_group: ShardGroup,
    current: &ValidatorSetFixture,
    next: &ValidatorSetFixture,
    signers: &Signers,
) -> EpochCheckpointEntry {
    checkpoint_entry(CheckpointSpec {
        shard_group,
        signers,
        next_epoch_committee: Some(next.committee(shard_group)),
        ..CheckpointSpec::handover(current, next)
    })
}

/// The checkpoint of `shard_group` ending `current`'s epoch that hands over to `next`, signed by `current`'s
/// committee of the group.
pub fn handover_checkpoint(
    shard_group: ShardGroup,
    current: &ValidatorSetFixture,
    next: &ValidatorSetFixture,
) -> EpochCheckpointEntry {
    checkpoint_signed_by(shard_group, current, next, current.signers(shard_group))
}

/// An indexer REST API serving the end-of-epoch checkpoints, committees and current epoch it is given. It answers
/// every other request with 404.
#[derive(Clone)]
pub(crate) struct FakeIndexer {
    pub url: url::Url,
    state: Arc<Mutex<FakeIndexerState>>,
}

#[derive(Default)]
struct FakeIndexerState {
    /// The latest checkpoint of the shard group the wallet walks the validator set handovers through.
    walked_checkpoint: Option<EpochCheckpointEntry>,
    /// The latest checkpoint of every shard group, in shard order.
    shard_group_checkpoints: Vec<EpochCheckpointEntry>,
    committees: Vec<ValidatorSetCommittee>,
    /// The epoch the indexer claims the network is in.
    current_epoch: Epoch,
}

impl FakeIndexer {
    pub async fn spawn() -> Self {
        let state = Arc::new(Mutex::new(FakeIndexerState::default()));
        let router = Router::new()
            .route("/epoch-checkpoints/latest", get(Self::latest_checkpoint))
            .route(
                "/epoch-checkpoints/latest/shard-groups",
                get(Self::latest_shard_group_checkpoints),
            )
            .route("/committees", get(Self::committee))
            .route("/epoch-manager/stats", get(Self::epoch_manager_stats))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = url::Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await });
        Self { url, state }
    }

    /// Serves the checkpoints of both shard groups ending `current`'s epoch, which hand over to `next`, as the latest
    /// ones, and `current`'s committees.
    pub fn end_epoch(&self, current: &ValidatorSetFixture, next: &ValidatorSetFixture) {
        let shard_groups = [low(), high()];
        let [low_checkpoint, high_checkpoint] =
            shard_groups.map(|shard_group| handover_checkpoint(shard_group, current, next));
        let mut state = self.state.lock().unwrap();
        state.walked_checkpoint = Some(low_checkpoint.clone());
        state.shard_group_checkpoints = vec![low_checkpoint, high_checkpoint];
        state
            .committees
            .extend(shard_groups.map(|shard_group| current.committee(shard_group)));
    }

    /// Lists `checkpoints` as the latest checkpoint of every shard group.
    pub fn list_shard_group_checkpoints(&self, checkpoints: Vec<EpochCheckpointEntry>) {
        self.state.lock().unwrap().shard_group_checkpoints = checkpoints;
    }

    pub fn claim_current_epoch(&self, epoch: Epoch) {
        self.state.lock().unwrap().current_epoch = epoch;
    }

    async fn latest_checkpoint(
        State(state): State<Arc<Mutex<FakeIndexerState>>>,
    ) -> Result<Json<GetLatestEpochCheckpointResponse>, StatusCode> {
        let checkpoint = state.lock().unwrap().walked_checkpoint.clone();
        checkpoint
            .map(|checkpoint| Json(GetLatestEpochCheckpointResponse { checkpoint }))
            .ok_or(StatusCode::NOT_FOUND)
    }

    async fn latest_shard_group_checkpoints(
        State(state): State<Arc<Mutex<FakeIndexerState>>>,
    ) -> Json<GetLatestShardGroupCheckpointsResponse> {
        Json(GetLatestShardGroupCheckpointsResponse {
            checkpoints: state.lock().unwrap().shard_group_checkpoints.clone(),
        })
    }

    async fn committee(
        State(state): State<Arc<Mutex<FakeIndexerState>>>,
        Query(req): Query<GetCommitteeRequest>,
    ) -> Result<Json<GetCommitteeResponse>, StatusCode> {
        state
            .lock()
            .unwrap()
            .committees
            .iter()
            .find(|committee| committee.epoch == req.epoch && committee.shard_group.contains(&req.shard))
            .map(|committee| {
                Json(GetCommitteeResponse {
                    committee: committee.clone(),
                })
            })
            .ok_or(StatusCode::NOT_FOUND)
    }

    async fn epoch_manager_stats(
        State(state): State<Arc<Mutex<FakeIndexerState>>>,
    ) -> Json<GetEpochManagerStatsResponse> {
        let current_epoch = state.lock().unwrap().current_epoch;
        Json(GetEpochManagerStatsResponse {
            current_epoch,
            consensus_epoch: Some(current_epoch),
            current_block_height: 0,
            current_block_hash: Default::default(),
        })
    }
}
