// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use std::{
    io,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use minotari_app_grpc::tari_rpc::GetActiveValidatorNodesResponse;
use tari_template_lib_types::crypto::{RistrettoPublicKeyBytes, SchnorrSignatureBytes};
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
    pub signature: SchnorrSignatureBytes,
    pub public_key: RistrettoPublicKeyBytes,
    pub claim_fees_public_key: RistrettoPublicKeyBytes,
}

pub async fn read_registration_file<P: AsRef<Path>>(
    vn_registration_file: P,
) -> anyhow::Result<Option<ValidatorNodeRegistration>> {
    log::debug!(
        "Using VN registration file at: {}",
        vn_registration_file.as_ref().display()
    );
    match fs::File::open(vn_registration_file).await {
        Ok(file) => {
            let mut file = file
                .try_into_std()
                .map_err(|_| anyhow!("Failed to convert async file to std file (inflight operations)"))?;
            let reg = serde_json5::from_reader(&mut file)?;
            Ok(Some(reg))
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => {
            log::error!("Failed to read VN registration file: {}", e);
            Err(e.into())
        },
    }
}

pub fn to_vn_public_keys(vns: Vec<GetActiveValidatorNodesResponse>) -> Vec<RistrettoPublicKeyBytes> {
    vns.into_iter()
        .map(|vn| RistrettoPublicKeyBytes::from_bytes(&vn.public_key).expect("Invalid public key, should not happen"))
        .collect()
}
