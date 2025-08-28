//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{path::Path, str::FromStr, time::Duration};

use log::info;
use tari_crypto::tari_utilities::SafePassword;
use tari_engine_types::commit_result::FinalizeResult;
use tari_ootle_common_types::Network;
use tari_ootle_wallet_sdk::{WalletSdk as Sdk, WalletSdkConfig};
use tari_ootle_wallet_storage_sqlite::SqliteWalletStore;
use tari_ootle_walletd::indexer_jrpc_impl::IndexerJsonRpcNetworkInterface;
use tari_transaction::{Transaction, TransactionBuilder, TransactionId};
use tari_validator_node_client::types::TemplateMetadata;
use tokio::time::sleep;
use url::Url;

use crate::{cli::CommonArgs, stats::Stats, templates::get_templates};

type WalletSdk = Sdk<SqliteWalletStore, IndexerJsonRpcNetworkInterface>;
pub struct Runner {
    pub(crate) sdk: WalletSdk,
    pub(crate) _cli: CommonArgs,
    pub(crate) faucet_template: TemplateMetadata,
    pub(crate) tariswap_template: TemplateMetadata,
    pub(crate) stats: Stats,
}

impl Runner {
    pub async fn init(cli: CommonArgs) -> anyhow::Result<Self> {
        let sdk = initialize_wallet_sdk(&cli.db_path, cli.indexer_url.clone())?;
        let (faucet_template, tariswap_template) = get_templates(&cli).await?;
        Ok(Self {
            sdk,
            _cli: cli,
            faucet_template,
            tariswap_template,
            stats: Stats::default(),
        })
    }

    pub async fn submit_transaction_and_wait(&mut self, transaction: Transaction) -> anyhow::Result<FinalizeResult> {
        let tx_id = self.submit_transaction(transaction).await?;
        let finalize = self.wait_for_transaction(tx_id).await?;
        Ok(finalize)
    }

    pub async fn submit_transaction(&mut self, transaction: Transaction) -> anyhow::Result<TransactionId> {
        let tx_id = self
            .sdk
            .transaction_api()
            .insert_new_transaction(transaction, None, false)
            .await?;

        self.sdk.transaction_api().submit_transaction(tx_id).await?;

        self.stats.inc_transaction();
        Ok(tx_id)
    }

    pub async fn wait_for_transaction(&mut self, tx_id: TransactionId) -> anyhow::Result<FinalizeResult> {
        loop {
            let Some(tx) = self
                .sdk
                .transaction_api()
                .check_and_store_finalized_transaction(tx_id)
                .await?
            else {
                sleep(Duration::from_secs(1)).await;
                continue;
            };

            let Some(ref finalize) = tx.finalize else {
                sleep(Duration::from_secs(1)).await;
                continue;
            };

            self.stats.add_execution_time(tx.execution_time.unwrap());
            self.stats.add_time_to_finalize(tx.finalized_time.unwrap());

            if !finalize.is_full_accept() {
                return Err(anyhow::anyhow!(
                    "Transaction {} failed: {:?}",
                    tx_id,
                    finalize.result.any_reject().unwrap()
                ));
            }

            self.stats
                .add_substate_created(finalize.result.any_accept().unwrap().up_len());

            return Ok(finalize.clone());
        }
    }

    pub fn log_stats(&self) {
        info!("Stats:");
        info!("  - Num transactions: {}", self.stats.num_transactions());
        info!("  - Total execution time: {:.2?}", self.stats.total_execution_time());
        info!(
            "  - Total time to finalize: {:.2?}",
            self.stats.total_time_to_finalize()
        );
        info!("  - Num substates created: {}", self.stats.num_substates_created());
    }

    pub fn new_transaction_builder(&self) -> TransactionBuilder {
        // 0x10 is LocalNet - avoiding having to include tari_common
        // Igor is 0x24
        Transaction::builder().for_network(0x10)
    }
}

fn initialize_wallet_sdk<P: AsRef<Path>>(db_path: P, indexer_url: Url) -> Result<WalletSdk, anyhow::Error> {
    let store = SqliteWalletStore::try_open(db_path)?;
    store.run_migrations()?;

    let sdk_config = WalletSdkConfig {
        network: Network::LocalNet,
        override_keyring_password: Some(SafePassword::from_str("N3Va g0nn4 gu355").unwrap()),
    };
    let indexer = IndexerJsonRpcNetworkInterface::new(indexer_url);
    let mut sdk = WalletSdk::initialize(store, indexer, sdk_config)?;
    sdk.initialize_cipher_seed(None)?;
    Ok(sdk)
}
