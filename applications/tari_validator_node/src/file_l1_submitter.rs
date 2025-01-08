//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io, path::PathBuf};

use log::*;
use rand::{rngs::OsRng, RngCore};
use serde::Serialize;
use tari_dan_common_types::layer_one_transaction::LayerOneTransactionDef;
use tari_epoch_manager::traits::LayerOneTransactionSubmitter;
use tokio::fs;

const LOG_TARGET: &str = "tari::validator_node::file_layer_one_submitter";

#[derive(Debug, Clone)]
pub struct FileLayerOneSubmitter {
    path: PathBuf,
}

impl FileLayerOneSubmitter {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl LayerOneTransactionSubmitter for FileLayerOneSubmitter {
    type Error = io::Error;

    async fn submit_transaction<T: Serialize + Send>(
        &self,
        proof: LayerOneTransactionDef<T>,
    ) -> Result<(), Self::Error> {
        fs::create_dir_all(&self.path).await?;
        let id = OsRng.next_u64();
        let file_name = format!("{}-{}.json", proof.proof_type, id);
        let path = self.path.join(file_name);
        info!(target: LOG_TARGET, "Saving layer one transaction proof to {}", path.display());
        let file = fs::File::create(path).await?;
        let mut file = file.into_std().await;
        serde_json::to_writer_pretty(&mut file, &proof)?;
        Ok(())
    }
}
