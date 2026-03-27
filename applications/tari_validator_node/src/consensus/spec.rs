//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_consensus::traits::ConsensusSpec;
#[cfg(not(feature = "metrics"))]
use tari_consensus::traits::hooks::NoopHooks;
use tari_engine::state_store::memory::ReadOnlyMemoryStateStore;
use tari_epoch_manager::service::EpochManagerHandle;
use tari_ootle_app_utilities::transaction_executor::TariTransactionProcessor;
use tari_ootle_p2p::PeerAddress;
use tari_rpc_state_sync::RpcStateSyncClientProtocol;

#[cfg(feature = "metrics")]
use crate::consensus::metrics::PrometheusConsensusMetrics;
use crate::{
    consensus::{
        ConsensusTransactionValidator,
        TarBlockTransactionExecutor,
        leader_selection::RoundRobinLeaderStrategy,
        signer_service::TariSignatureService,
    },
    p2p::{
        NopLogger,
        services::messaging::{ConsensusInboundMessaging, ConsensusOutboundMessaging},
    },
    state_store_template_provider::StateStoreTemplateProvider,
};

pub type ValidatorNodeStateStore = tari_state_store_rocksdb::RocksDbStateStore<PeerAddress>;
pub type ValidatorTransactionProcessor =
    TariTransactionProcessor<ReadOnlyMemoryStateStore, StateStoreTemplateProvider<ValidatorNodeStateStore>>;
#[derive(Clone)]
pub struct TariConsensusSpec;

impl ConsensusSpec for TariConsensusSpec {
    type Addr = PeerAddress;
    type EpochManager = EpochManagerHandle<Self::Addr>;
    #[cfg(not(feature = "metrics"))]
    type Hooks = NoopHooks;
    #[cfg(feature = "metrics")]
    type Hooks = PrometheusConsensusMetrics;
    type InboundMessaging = ConsensusInboundMessaging<NopLogger>;
    type LeaderStrategy = RoundRobinLeaderStrategy;
    type OutboundMessaging = ConsensusOutboundMessaging<NopLogger>;
    type SignerService = TariSignatureService;
    type StateStore = ValidatorNodeStateStore;
    type SyncManager = RpcStateSyncClientProtocol<Self>;
    type TransactionExecutor =
        TarBlockTransactionExecutor<ValidatorTransactionProcessor, ConsensusTransactionValidator>;
}
