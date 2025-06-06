//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

#[cfg(not(feature = "metrics"))]
use tari_consensus::traits::hooks::NoopHooks;
use tari_consensus::traits::ConsensusSpec;
use tari_epoch_manager::service::EpochManagerHandle;
use tari_ootle_app_utilities::transaction_executor::TariTransactionProcessor;
use tari_ootle_common_types::PeerAddress;
use tari_rpc_state_sync::RpcStateSyncClientProtocol;
use tari_template_manager::implementation::TemplateManager;

#[cfg(feature = "metrics")]
use crate::consensus::metrics::PrometheusConsensusMetrics;
use crate::{
    consensus::{
        leader_selection::RoundRobinLeaderStrategy,
        signer_service::TariSignatureService,
        ConsensusTransactionValidator,
        TarBlockTransactionExecutor,
    },
    p2p::{
        services::messaging::{ConsensusInboundMessaging, ConsensusOutboundMessaging},
        NopLogger,
    },
};

pub type ValidatorNodeStateStore = tari_state_store_rocksdb::RocksDbStateStore<PeerAddress>;
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
    type TransactionExecutor = TarBlockTransactionExecutor<
        TariTransactionProcessor<TemplateManager<PeerAddress>>,
        ConsensusTransactionValidator,
    >;
}
