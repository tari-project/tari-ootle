//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io, path::PathBuf};

use log::*;
use rand::Rng;
use serde::Serialize;
use tari_epoch_manager::traits::LayerOneTransactionSubmitter;
use tari_ootle_common_types::layer_one_transaction::LayerOneTransactionDef;
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
    type Output = PathBuf;

    async fn submit_transaction<T: Serialize + Send>(
        &self,
        transaction: LayerOneTransactionDef<T>,
    ) -> Result<Self::Output, Self::Error> {
        fs::create_dir_all(&self.path).await?;
        let json = serde_json::to_vec_pretty(&transaction).map_err(io::Error::other)?;
        let id = rand::rng().next_u64();
        let file_name = format!("{}-{}.json", transaction.payload_type, id);
        let path = self.path.join(&file_name);
        let tmp_path = self.path.join(format!(".{file_name}.tmp"));
        info!(target: LOG_TARGET, "Saving layer transaction to {}", path.display());
        fs::write(&tmp_path, &json).await?;
        if let Err(err) = fs::rename(&tmp_path, &path).await {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(err);
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use tari_ootle_common_types::{
        Epoch,
        layer_one_transaction::{LayerOnePayloadType, ValidatorExitParams},
    };
    use tari_template_lib::types::crypto::{RistrettoPublicKeyBytes, Scalar32Bytes, SchnorrSignatureBytes};

    use super::*;

    #[tokio::test]
    async fn submit_transaction_writes_complete_json_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let out_dir = dir.path().join("l1_txs");
        let submitter = FileLayerOneSubmitter::new(out_dir.clone());

        let tx_def = LayerOneTransactionDef {
            payload_type: LayerOnePayloadType::ValidatorExit,
            payload: ValidatorExitParams {
                public_key: RistrettoPublicKeyBytes::default(),
                signature: SchnorrSignatureBytes::new(RistrettoPublicKeyBytes::default(), Scalar32Bytes::default()),
                activation_epoch: Epoch(1),
                max_epoch: Epoch(4),
                sidechain_public_key: None,
            },
        };

        let written_path = submitter.submit_transaction(tx_def).await.unwrap();
        assert!(written_path.exists());

        let parsed: LayerOneTransactionDef<ValidatorExitParams> =
            serde_json::from_slice(&std::fs::read(&written_path).unwrap()).unwrap();
        assert_eq!(parsed.payload_type, LayerOnePayloadType::ValidatorExit);
        assert_eq!(parsed.payload.activation_epoch, Epoch(1));
        assert_eq!(parsed.payload.max_epoch, Epoch(4));

        let entries: Vec<_> = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(entries, vec![written_path]);
    }
}
