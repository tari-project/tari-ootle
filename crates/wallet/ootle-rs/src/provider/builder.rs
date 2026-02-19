//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_indexer_client::error::IndexerRestClientError;
use tari_ootle_common_types::Network;

use crate::{
    provider::indexer::IndexerProvider,
    wallet::{NetworkWallet, NoWallet},
};

#[derive(Debug)]
pub struct ProviderBuilder<Wallet = NoWallet> {
    wallet: Wallet,
    network: Network,
}

impl ProviderBuilder<NoWallet> {
    pub fn new() -> Self {
        Self {
            network: Network::MainNet,
            wallet: NoWallet,
        }
    }
}

impl<Wallet> ProviderBuilder<Wallet> {
    pub fn with_network(mut self, network: Network) -> Self {
        self.network = network;
        self
    }

    pub fn wallet<W: NetworkWallet>(self, wallet: W) -> ProviderBuilder<W> {
        ProviderBuilder {
            network: wallet.default_address().network(),
            wallet,
        }
    }

    pub async fn connect<T: AsRef<str>>(self, url: T) -> Result<IndexerProvider<Wallet>, IndexerRestClientError> {
        let client = tari_indexer_client::connect_rest(url.as_ref())?;
        Ok(IndexerProvider::new(client, self.wallet, self.network))
    }
}

impl Default for ProviderBuilder<NoWallet> {
    fn default() -> Self {
        Self::new()
    }
}
