//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus::traits::{hooks::NoopHooks, ConsensusSpec};

use super::TestBlockTransactionProcessor;
use crate::support::{
    address::TestAddress,
    epoch_manager::TestEpochManager,
    messaging_impls::{TestInboundMessaging, TestOutboundMessaging},
    signing_service::TestVoteSignatureService,
    sync::AlwaysSyncedSyncManager,
    RoundRobinLeaderStrategy,
};

#[cfg(not(feature = "sqlite_backend"))]
pub type TestStore = tari_state_store_rocksdb::RocksDbStateStore<TestAddress>;
#[cfg(feature = "sqlite_backend")]
pub type TestStore = tari_state_store_sqlite::SqliteStateStore<TestAddress>;

#[derive(Clone)]
pub struct TestConsensusSpec;

impl ConsensusSpec for TestConsensusSpec {
    type Addr = TestAddress;
    type EpochManager = TestEpochManager;
    type Hooks = NoopHooks;
    type InboundMessaging = TestInboundMessaging;
    type LeaderStrategy = RoundRobinLeaderStrategy;
    type OutboundMessaging = TestOutboundMessaging;
    type SignatureService = TestVoteSignatureService;
    type StateStore = TestStore;
    type SyncManager = AlwaysSyncedSyncManager;
    type TransactionExecutor = TestBlockTransactionProcessor;
}
