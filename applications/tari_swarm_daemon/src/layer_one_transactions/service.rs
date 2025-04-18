//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fs,
    path::{Path, PathBuf},
};

use minotari_wallet_grpc_client::WalletGrpcClient;
use serde::de::DeserializeOwned;
use tari_dan_common_types::layer_one_transaction::LayerOneTransactionDef;

use super::submitter;

pub struct LayerOneTransactionService {
    watch_list: Vec<PathBuf>,
    submitter: submitter::LayerOneTransactionSubmitter,
}

impl LayerOneTransactionService {
    pub fn init(wallet_client: WalletGrpcClient<tonic::transport::Channel>) -> anyhow::Result<Self> {
        let submitter = submitter::LayerOneTransactionSubmitter::new(wallet_client);
        Ok(Self {
            watch_list: vec![],
            submitter,
        })
    }

    pub fn add_watch<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref();
        assert!(path.is_dir(), "watch path must be a directory");
        self.watch_list.push(path.to_path_buf());
    }

    fn find_new_transaction_files(&self) -> anyhow::Result<Vec<PathBuf>> {
        let mut new_files = vec![];
        for path in &self.watch_list {
            if !path.exists() {
                continue;
            }
            let entries = fs::read_dir(path)?;
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "json") {
                    new_files.push(path);
                }
            }
        }
        Ok(new_files)
    }

    /// Processes any new files in the watched directory.
    /// This should be called repeatedly to check for and process new transaction files.
    /// If a file is successfully processed, it will be renamed to have a `.processed` extension.
    /// If processing fails, it will be renamed to have a `.failed` extension.
    /// Returns a vector of tuples containing the transaction and its ID.
    pub async fn process_any(&mut self) -> anyhow::Result<Vec<(LayerOneTransactionDef<serde_json::Value>, u64)>> {
        let mut processed = vec![];
        let new_files = self.find_new_transaction_files()?;

        for path in new_files {
            log::info!("Processing file: {}", path.display());
            let transaction: LayerOneTransactionDef<serde_json::Value> = decode_json_file(&path)?;
            match self.submitter.submit_transaction(transaction.clone()).await {
                Ok(tx_id) => {
                    log::info!("Transaction submitted successfully: {}", path.display());
                    fs::rename(&path, path.with_extension("json.processed"))?;
                    processed.push((transaction, tx_id));
                },
                Err(err) => {
                    log::error!("Failed to submit transaction: {}", err);
                    fs::rename(&path, path.with_extension("json.failed"))?;
                    return Err(err);
                },
            }
        }

        Ok(processed)
    }
}

fn decode_json_file<P: AsRef<Path>, T: DeserializeOwned>(path: P) -> anyhow::Result<T> {
    let file = fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let t = serde_json::from_reader(reader)?;
    Ok(t)
}
