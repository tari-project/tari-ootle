//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock, Weak},
    time::Duration,
};

use tari_indexer_client::{
    rest_api_client::IndexerRestApiClient,
    types::{GetSubstateRequest, GetSubstatesRequest, SubmitTransactionRequest},
};
use tari_ootle_common_types::{
    Epoch,
    Network,
    engine_types::{
        commit_result::ExecuteResult,
        substate::{Substate, SubstateId},
    },
    optional::Optional,
};
use tari_ootle_transaction::{Transaction, TransactionEnvelope, UnsignedTransaction};
use tracing::debug;

use crate::{
    Address,
    provider::{
        PendingTransaction,
        Provider,
        ProviderError,
        ProviderResult,
        TransactionEventFilter,
        TransactionEventStream,
        TransactionWatcher,
        WalletProvider,
        WantInput,
        input_resolver::TransactionInputResolver,
        tx_stream::{EventStream, Paused},
        tx_watcher::TransactionWatcherHandle,
    },
    wallet::NetworkWallet,
};

#[derive(Debug, Clone)]
pub struct IndexerProvider<Wallet> {
    client: Arc<IndexerRestApiClient>,
    wallet: Wallet,
    network: Network,
    tx_timeout: Duration,
    tx_watcher: Arc<OnceLock<TransactionWatcherHandle>>,
}

impl<Wallet> IndexerProvider<Wallet> {
    pub fn new(client: IndexerRestApiClient, wallet: Wallet, network: Network) -> Self {
        Self {
            client: Arc::new(client),
            wallet,
            network,
            tx_timeout: Duration::from_secs(60),
            tx_watcher: Arc::new(OnceLock::new()),
        }
    }

    /// The default timeout when waiting for a transaction to be finalized. This is used by the `PendingTransaction`
    /// returned by `send_transaction`.
    pub fn with_transaction_timeout(mut self, timeout: Duration) -> Self {
        self.tx_timeout = timeout;
        self
    }

    pub(crate) fn client(&self) -> &IndexerRestApiClient {
        &self.client
    }

    pub async fn get_network(&self) -> ProviderResult<Network> {
        let resp = self.client.get_network_info().await?;
        Ok(resp.network)
    }

    pub async fn fetch_substate<T: Into<SubstateId>>(&self, substate_id: T) -> ProviderResult<Substate> {
        let resp = self
            .client
            .get_substate(&substate_id.into(), GetSubstateRequest::default())
            .await?;
        Ok(Substate::new(resp.version, resp.substate))
    }

    pub async fn get_epoch(&self) -> ProviderResult<Epoch> {
        let resp = self.client.get_network_info().await?;
        Ok(resp.epoch)
    }

    /// Subscribe to a filtered stream of template-emitted events via SSE.
    /// Each event is a template `Event` paired with its originating `TransactionId`.
    ///
    /// This is independent from transaction finalization watching (`PendingTransaction`).
    pub fn watch_events(&self, filter: TransactionEventFilter) -> TransactionEventStream {
        TransactionEventStream::new(Arc::downgrade(&self.client), filter)
    }

    pub async fn get_substate<T: Into<SubstateId>>(&self, substate_id: T) -> ProviderResult<Option<Substate>> {
        let resp = self
            .client
            .get_substate(&substate_id.into(), GetSubstateRequest::default())
            .await
            .optional()?;
        Ok(resp.map(|r| Substate::new(r.version, r.substate)))
    }
}

impl<Wallet: NetworkWallet + Send + Sync> IndexerProvider<Wallet> {
    pub async fn sign_and_send_dry_run(&self, unsigned: UnsignedTransaction) -> ProviderResult<ExecuteResult> {
        self.sign_and_send_dry_run_with(self.wallet(), unsigned).await
    }

    pub async fn sign_and_send_dry_run_with<W: NetworkWallet>(
        &self,
        wallet: &W,
        unsigned: UnsignedTransaction,
    ) -> ProviderResult<ExecuteResult> {
        let unsigned = unsigned.with_dry_run(true);
        let transaction = wallet.sign_transaction(unsigned).await?;
        debug!("Submitting dry-run transaction: {}", transaction.calculate_id());
        self.send_dry_run(transaction).await
    }

    pub async fn send_dry_run(&self, tx: Transaction) -> ProviderResult<ExecuteResult> {
        let resp = self
            .client
            .submit_transaction_dry_run(SubmitTransactionRequest {
                transaction: TransactionEnvelope::encode(tx)?,
            })
            .await?;
        Ok(resp.result)
    }

    pub async fn send_transaction(&mut self, transaction: Transaction) -> ProviderResult<PendingTransaction> {
        debug!("Sending transaction: {}", transaction.calculate_id());
        let envelope = TransactionEnvelope::encode(transaction)?;
        self.send_transaction_envelope(envelope).await
    }

    pub async fn send_transaction_envelope(
        &mut self,
        transaction: TransactionEnvelope,
    ) -> ProviderResult<PendingTransaction> {
        // Start the tx watcher if not already started
        let watcher = self.get_tx_watcher().clone();

        let resp = self
            .client
            .submit_transaction(SubmitTransactionRequest { transaction })
            .await?;
        Ok(PendingTransaction::new(watcher, self.weak_client(), resp.transaction_id).with_timeout(self.tx_timeout))
    }

    pub(crate) fn get_tx_watcher(&self) -> &TransactionWatcherHandle {
        self.tx_watcher.get_or_init(|| {
            let paused = Paused::default();
            let event_stream = EventStream::new(self.weak_client(), paused.waiter());
            let watcher = TransactionWatcher::new(Box::pin(event_stream.into_stream()), paused);
            watcher.spawn()
        })
    }
}

impl<Wallet: NetworkWallet + Send + Sync> Provider for IndexerProvider<Wallet> {
    type Client = IndexerRestApiClient;

    fn network(&self) -> Network {
        self.network
    }

    fn weak_client(&self) -> Weak<Self::Client> {
        Arc::downgrade(&self.client)
    }

    fn default_signer_address(&self) -> &Address {
        self.wallet.default_address()
    }

    async fn resolve_input_want_list(
        &self,
        mut transaction: UnsignedTransaction,
        want_list: &HashSet<WantInput>,
    ) -> ProviderResult<UnsignedTransaction> {
        TransactionInputResolver::new(self.weak_client())
            .resolve_inputs(&mut transaction, want_list)
            .await?;
        Ok(transaction)
    }

    async fn fetch_substates<I: IntoIterator<Item = SubstateId> + Send>(
        &self,
        substate_ids: I,
    ) -> ProviderResult<HashMap<SubstateId, Substate>> {
        let substate_ids = substate_ids.into_iter().collect::<Vec<_>>();
        if substate_ids.is_empty() {
            // The request API will not allow us to send an empty list, so we short-circuit here
            return Ok(HashMap::new());
        }

        let resp = self
            .client
            .fetch_substates(GetSubstatesRequest {
                requests: substate_ids
                    .try_into()
                    .map_err(|_| ProviderError::other("Too many substates requested in single request"))?,
                cached_only: false,
            })
            .await?;

        Ok(resp.substates)
    }
}

impl<Wallet: NetworkWallet + Send + Sync> WalletProvider for IndexerProvider<Wallet> {
    type Wallet = Wallet;

    fn wallet(&self) -> &Self::Wallet {
        &self.wallet
    }

    fn wallet_mut(&mut self) -> &mut Self::Wallet {
        &mut self.wallet
    }
}
