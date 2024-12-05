// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{
    io,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use minotari_app_grpc::tari_rpc::GetActiveValidatorNodesResponse;
use tari_common_types::types::PublicKey;
use tari_core::transactions::transaction_components::ValidatorNodeSignature;
use tari_crypto::tari_utilities::ByteArray;
use tokio::fs;

use crate::config::Config;

pub async fn read_config_file(path: PathBuf) -> anyhow::Result<Config> {
    let content = fs::read_to_string(&path)
        .await
        .map_err(|_| anyhow!("Failed to read config file at {}", path.display()))?;

    let config = toml::from_str(&content)?;

    Ok(config)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ValidatorNodeRegistration {
    pub signature: ValidatorNodeSignature,
    pub public_key: PublicKey,
    pub claim_fees_public_key: PublicKey,
}

pub async fn read_registration_file<P: AsRef<Path>>(
    vn_registration_file: P,
) -> anyhow::Result<Option<ValidatorNodeRegistration>> {
    log::debug!(
        "Using VN registration file at: {}",
        vn_registration_file.as_ref().display()
    );
    match fs::read_to_string(vn_registration_file).await {
        Ok(info) => {
            let reg = json5::from_str(&info)?;
            Ok(Some(reg))
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => {
            log::error!("Failed to read VN registration file: {}", e);
            Err(e.into())
        },
    }
}

pub fn to_vn_public_keys(vns: Vec<GetActiveValidatorNodesResponse>) -> Vec<PublicKey> {
    vns.into_iter()
        .map(|vn| PublicKey::from_vec(&vn.public_key).expect("Invalid public key, should not happen"))
        .collect()
}
