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
        .filter_map(|vn| match RistrettoPublicKeyBytes::from_bytes(&vn.public_key) {
            Ok(pk) => Some(pk),
            Err(err) => {
                log::warn!("Skipping malformed validator public key from base node: {err}");
                None
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_vn_public_keys_skips_malformed_keys_instead_of_panicking() {
        let valid_bytes = [7u8; 32];
        let vns = vec![
            GetActiveValidatorNodesResponse {
                public_key: vec![1, 2, 3],
                ..Default::default()
            },
            GetActiveValidatorNodesResponse {
                public_key: valid_bytes.to_vec(),
                ..Default::default()
            },
        ];

        let keys = to_vn_public_keys(vns);
        assert_eq!(keys, vec![RistrettoPublicKeyBytes::from_bytes(&valid_bytes).unwrap()]);
    }
}
