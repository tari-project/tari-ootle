//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashSet,
    sync::{Arc, OnceLock, Weak},
};

use tari_indexer_client::{
    rest_api_client::IndexerRestApiClient,
    types::{GetSubstateRequest, SubmitTransactionRequest},
};
use tari_ootle_common_types::{
    engine_types::{
        commit_result::ExecuteResult,
        substate::{Substate, SubstateId},
    },
    optional::Optional,
    Epoch,
};
use tari_ootle_transaction::{Transaction, TransactionEnvelope, UnsignedTransaction};
use tari_ootle_wallet_sdk::Network;
use tracing::debug;

use crate::{
    provider::{
        input_resolver::TransactionInputResolver,
        tx_stream::{EventStream, Paused},
        tx_watcher::TransactionWatcherHandle,
        PendingTransactionHandle,
        Provider,
        ProviderResult,
        TransactionWatcher,
        WantInput,
    },
    wallet::OotleWallet,
    Address,
};

#[derive(Debug, Clone)]
pub struct IndexerProvider<Wallet> {
    client: Arc<IndexerRestApiClient>,
    wallet: Wallet,
    network: Network,
    tx_watcher: Arc<OnceLock<TransactionWatcherHandle>>,
}

impl<Wallet> IndexerProvider<Wallet> {
    pub fn new(client: IndexerRestApiClient, wallet: Wallet, network: Network) -> Self {
        Self {
            client: Arc::new(client),
            wallet,
            network,
            tx_watcher: Arc::new(OnceLock::new()),
        }
    }

    pub fn wallet(&self) -> &Wallet {
        &self.wallet
    }

    pub fn wallet_mut(&mut self) -> &mut Wallet {
        &mut self.wallet
    }

    pub async fn get_network(&self) -> ProviderResult<Network> {
        let resp = self.client.get_network_info().await?;
        Ok(resp.network)
    }

    pub async fn get_epoch(&self) -> ProviderResult<Epoch> {
        let resp = self.client.get_network_info().await?;
        Ok(resp.epoch)
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

impl IndexerProvider<OotleWallet> {
    pub async fn send_dry_run(&self, transaction: UnsignedTransaction) -> ProviderResult<ExecuteResult> {
        let transaction = transaction.with_dry_run(true);
        // Sign the transaction - TODO: use invalid signatures for fee estimation since they are not validated anyway
        let mut signatures = vec![];
        for signer in self.wallet().additional_signers() {
            let sig = self.wallet().authorize_transaction(signer, &transaction).await?;
            signatures.push(sig.into_signature());
        }
        let transaction = self
            .wallet()
            .sign_transaction(transaction.with_signatures(signatures))
            .await?;

        debug!("Submitting dry-run transaction: {}", transaction.calculate_id());

        let resp = self
            .client
            .submit_transaction_dry_run(SubmitTransactionRequest {
                transaction: TransactionEnvelope::encode(transaction)?,
            })
            .await?;
        Ok(resp.result)
    }

    pub async fn send_transaction(&mut self, transaction: Transaction) -> ProviderResult<PendingTransactionHandle> {
        debug!("Sending transaction: {}", transaction.calculate_id());
        let envelope = TransactionEnvelope::encode(transaction)?;
        self.send_transaction_envelope(envelope).await
    }

    pub async fn send_transaction_envelope(
        &mut self,
        transaction: TransactionEnvelope,
    ) -> ProviderResult<PendingTransactionHandle> {
        // Start the tx watcher if not already started
        let watcher = self.get_tx_watcher().clone();

        let resp = self
            .client
            .submit_transaction(SubmitTransactionRequest { transaction })
            .await?;
        Ok(PendingTransactionHandle::new(
            watcher,
            self.weak_client(),
            resp.transaction_id,
        ))
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

impl Provider for IndexerProvider<OotleWallet> {
    type Client = IndexerRestApiClient;

    fn network(&self) -> Network {
        self.network
    }

    fn weak_client(&self) -> Weak<Self::Client> {
        Arc::downgrade(&self.client)
    }

    fn default_signer_address(&self) -> &Address {
        self.wallet.default_signer_address()
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
}
