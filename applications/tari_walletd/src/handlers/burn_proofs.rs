//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::Path;

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use log::*;
use tari_ootle_walletd_client::{
    permissions::{Permission, ReadOnly},
    types::{
        BurnProofFileInfo,
        BurnProofsGetRequest,
        BurnProofsGetResponse,
        BurnProofsListRequest,
        BurnProofsListResponse,
        ClaimBurnProofContents,
    },
};
use tari_sidechain::CompleteClaimBurnProof;

use super::{
    context::HandlerContext,
    helpers::{MAX_BURN_PROOF_BYTES, complete_burn_proof_to_contents, validate_burn_proof_file_name},
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::burn_proofs";

/// Extracts the public key prefix from a burn proof file name with format `{pubkey}-{commitment}.json`.
/// Returns `None` if the file name does not match this format.
fn extract_public_key_prefix(file_name: &str) -> Option<&str> {
    let stem = file_name.strip_suffix(".json")?;
    let (pk, _commitment) = stem.split_once('-')?;
    if pk.is_empty() {
        return None;
    }
    Some(pk)
}

pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: BurnProofsListRequest,
) -> Result<BurnProofsListResponse, anyhow::Error> {
    context.authorize(token, &[Permission::BurnProofs(ReadOnly::Read)])?;
    let dir = context.config().get_burn_proof_dir(context.wallet_sdk().network());
    let proofs = list_burn_proofs(&dir, req.filter_by_public_key.as_deref()).await;
    Ok(BurnProofsListResponse { proofs })
}

async fn list_burn_proofs(dir: &Path, filter_by_public_key: Option<&str>) -> Vec<BurnProofFileInfo> {
    let mut read_dir = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Vec::new();
        },
        Err(e) => {
            warn!(target: LOG_TARGET, "Failed to read burn proofs directory {}: {}", dir.display(), e);
            return Vec::new();
        },
    };

    let mut proofs = Vec::new();
    loop {
        let entry = match read_dir.next_entry().await {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(e) => {
                warn!(target: LOG_TARGET, "Failed to read directory entry: {}", e);
                continue;
            },
        };

        let path = entry.path();
        let metadata = match entry.metadata().await {
            Ok(metadata) => metadata,
            Err(e) => {
                warn!(target: LOG_TARGET, "Failed to read metadata for {}: {}", path.display(), e);
                continue;
            },
        };
        if !metadata.is_file() || path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Apply account public key filter if provided
        if let Some(filter_pk) = filter_by_public_key {
            // A file named `{pubkey}-{commitment}.json` must name the filter key; a parsed proof must be for it too.
            if let Some(file_pk) = extract_public_key_prefix(name) && !file_pk.eq_ignore_ascii_case(filter_pk) {
                continue;
            }
        }

        // Read value from the proof file
        let value = if metadata.len() > MAX_BURN_PROOF_BYTES {
            warn!(
                target: LOG_TARGET,
                "Burn proof {} exceeds {} bytes ({} bytes)",
                path.display(),
                MAX_BURN_PROOF_BYTES,
                metadata.len()
            );
            None
        } else {
            match tokio::fs::read(&path).await {
                Ok(bytes) => match serde_json::from_slice::<CompleteClaimBurnProof>(&bytes) {
                    Ok(proof) => {
                        if let Some(filter_pk) = filter_by_public_key &&
                            !proof
                                .claim_proof
                                .burn_public_key
                                .to_string()
                                .eq_ignore_ascii_case(filter_pk)
                        {
                            continue;
                        }
                        Some(proof.claim_proof.value)
                    },
                    Err(e) => {
                        warn!(target: LOG_TARGET, "Failed to parse burn proof {}: {}", path.display(), e);
                        None
                    },
                },
                Err(e) => {
                    warn!(target: LOG_TARGET, "Failed to read burn proof {}: {}", path.display(), e);
                    None
                },
            }
        };

        if filter_by_public_key.is_some() && value.is_none() {
            continue;
        }

        proofs.push(BurnProofFileInfo {
            file_name: name.to_string(),
            value,
        });
    }

    proofs.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    proofs
}

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: BurnProofsGetRequest,
) -> Result<BurnProofsGetResponse, anyhow::Error> {
    context.authorize(token, &[Permission::BurnProofs(ReadOnly::Read)])?;
    let dir = context.config().get_burn_proof_dir(context.wallet_sdk().network());
    let proof = get_burn_proof(&dir, &req.file_name).await?;
    Ok(BurnProofsGetResponse { proof })
}

async fn get_burn_proof(dir: &Path, file_name: &str) -> Result<ClaimBurnProofContents, anyhow::Error> {
    validate_burn_proof_file_name(file_name)?;

    let path = dir.join(file_name);
    let metadata = tokio::fs::metadata(&path).await.map_err(|e| {
        warn!(target: LOG_TARGET, "Failed to read burn proof file {}: {}", path.display(), e);
        anyhow!("Burn proof file not found: {file_name}")
    })?;
    if !metadata.is_file() {
        return Err(anyhow!("Burn proof file not found: {file_name}"));
    }
    if metadata.len() > MAX_BURN_PROOF_BYTES {
        return Err(anyhow!("Burn proof file too large: {file_name}"));
    }

    let bytes = tokio::fs::read(&path).await.map_err(|e| {
        warn!(target: LOG_TARGET, "Failed to read burn proof file {}: {}", path.display(), e);
        anyhow!("Burn proof file not found: {file_name}")
    })?;

    let complete_proof: CompleteClaimBurnProof = serde_json::from_slice(&bytes).map_err(|e| {
        warn!(target: LOG_TARGET, "Failed to parse burn proof file {}: {}", path.display(), e);
        anyhow!("Invalid burn proof file: {file_name}")
    })?;

    complete_burn_proof_to_contents(complete_proof)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn write_file(dir: &Path, name: &str, bytes: &[u8]) {
        tokio::fs::write(dir.join(name), bytes).await.unwrap();
    }

    #[test]
    fn extract_public_key_prefix_parses_hyphen_format() {
        assert_eq!(
            extract_public_key_prefix("0123456789abcdef-deadbeef.json"),
            Some("0123456789abcdef")
        );
        assert_eq!(extract_public_key_prefix("custom.json"), None);
        assert_eq!(extract_public_key_prefix("-deadbeef.json"), None);
    }

    #[tokio::test]
    async fn list_burn_proofs_skips_non_json_and_unverified_filtered_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let oversized = vec![b' '; (MAX_BURN_PROOF_BYTES as usize) + 1];

        write_file(dir, "README", b"not a json file").await;
        write_file(dir, "notes.txt", b"not a json file").await;
        tokio::fs::create_dir(dir.join("claimed")).await.unwrap();
        write_file(dir, "custom.json", b"not valid json").await;
        write_file(dir, "aaaa-1111.json", b"not valid json").await;
        write_file(dir, "oversized.json", &oversized).await;

        let unfiltered = list_burn_proofs(dir, None).await;
        let names: Vec<_> = unfiltered.iter().map(|i| i.file_name.as_str()).collect();
        assert_eq!(names, vec!["aaaa-1111.json", "custom.json", "oversized.json"]);
        assert!(unfiltered.iter().all(|info| info.value.is_none()));

        let filtered = list_burn_proofs(dir, Some("aaaa")).await;
        assert!(filtered.is_empty());
    }

    #[tokio::test]
    async fn get_burn_proof_rejects_invalid_names_directories_and_oversized_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let oversized = vec![b' '; (MAX_BURN_PROOF_BYTES as usize) + 1];

        tokio::fs::create_dir(dir.join("claimed")).await.unwrap();
        write_file(dir, "oversized.json", &oversized).await;

        let err = get_burn_proof(dir, ".").await.unwrap_err();
        assert!(err.to_string().contains("Invalid file name"), "{err}");

        let err = get_burn_proof(dir, "claimed").await.unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");

        let err = get_burn_proof(dir, "oversized.json").await.unwrap_err();
        assert!(err.to_string().contains("too large"), "{err}");
    }
}
