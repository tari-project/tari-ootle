//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::sync::Arc;

use axum::{
    http,
    response::{IntoResponse, Response},
};
use tari_engine_types::ToByteType;
use tari_epoch_manager::service::EpochManagerHandle;
use tari_networking::NetworkingHandle;
use tari_ootle_common_types::PeerAddress;
use tari_ootle_p2p::TariMessagingSpec;
use tari_ootle_storage::global::GlobalDb;
use tari_ootle_storage_sqlite::global::SqliteGlobalDbAdapter;
use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;
use tari_validator_node_rpc::client::TariValidatorNodeRpcClientFactory;
use tokio::sync::broadcast;

use crate::{
    bootstrap::Services,
    dry_run::processor::DryRunTransactionProcessor,
    event::IndexerEvent,
    notify::Subscriber,
    rest_api::cache::HttpCacheConfig,
    storage_sqlite::SqliteIndexerStore,
    store::ReadOnlyStore,
    substate_manager::SubstateManager,
    template_manager::TemplateManager,
    transaction_manager::TransactionManager,
};

#[derive(Clone)]
pub struct HandlerContext {
    inner: Arc<InnerContext>,
}

impl HandlerContext {
    pub fn from_services(services: &Services) -> Self {
        Self {
            inner: Arc::new(InnerContext {
                cache_control_enabled: true,
                read_only_store: ReadOnlyStore::new(services.store.clone()),
                global_db: services.global_db.clone(),
                epoch_manager: services.epoch_manager.clone(),
                networking: services.networking.clone(),
                public_key: services.keypair.public_key().to_byte_type(),
                substate_manager: services.substate_manager.clone(),
                transaction_manager: services.transaction_manager.clone(),
                template_manager: services.template_manager.clone(),
                dry_run_transaction_processor: services.dry_run_transaction_processor.clone(),
                subscriber: services.event_notifier.to_subscriber(),
            }),
        }
    }

    pub fn global_db(&self) -> &GlobalDb<SqliteGlobalDbAdapter<PeerAddress>> {
        &self.inner.global_db
    }

    pub fn epoch_manager(&self) -> &EpochManagerHandle<PeerAddress> {
        &self.inner.epoch_manager
    }

    pub fn networking(&self) -> &NetworkingHandle<TariMessagingSpec> {
        &self.inner.networking
    }

    pub fn public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.inner.public_key
    }

    pub fn is_cache_control_enabled(&self) -> bool {
        self.inner.cache_control_enabled
    }

    pub fn substate_manager(&self) -> &SubstateManager {
        &self.inner.substate_manager
    }

    pub fn transaction_manager(
        &self,
    ) -> &TransactionManager<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory, SqliteIndexerStore>
    {
        &self.inner.transaction_manager
    }

    pub fn read_only_store(&self) -> &ReadOnlyStore<SqliteIndexerStore> {
        &self.inner.read_only_store
    }

    pub fn template_manager(&self) -> &TemplateManager {
        &self.inner.template_manager
    }

    pub fn dry_run_transaction_processor(&self) -> &DryRunTransactionProcessor {
        &self.inner.dry_run_transaction_processor
    }

    pub fn apply_cache_control(&self, body: impl IntoResponse, max_age: u32) -> Response {
        let mut response = body.into_response();
        let headers = response.headers_mut();
        self.apply_custom_cache_control(headers, &HttpCacheConfig::new().with_max_age(max_age));
        response
    }

    pub fn apply_custom_cache_control(&self, headers: &mut http::HeaderMap, config: &HttpCacheConfig) {
        if self.is_cache_control_enabled() {
            config.apply(headers);
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<IndexerEvent> {
        self.inner.subscriber.subscribe()
    }
}

struct InnerContext {
    cache_control_enabled: bool,
    global_db: GlobalDb<SqliteGlobalDbAdapter<PeerAddress>>,
    epoch_manager: EpochManagerHandle<PeerAddress>,
    networking: NetworkingHandle<TariMessagingSpec>,
    public_key: RistrettoPublicKeyBytes,
    substate_manager: SubstateManager,
    transaction_manager:
        TransactionManager<EpochManagerHandle<PeerAddress>, TariValidatorNodeRpcClientFactory, SqliteIndexerStore>,
    read_only_store: ReadOnlyStore<SqliteIndexerStore>,
    template_manager: TemplateManager,
    dry_run_transaction_processor: DryRunTransactionProcessor,
    subscriber: Subscriber<IndexerEvent>,
}
