//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

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
    },
};
use tari_sidechain::CompleteClaimBurnProof;

use super::{
    context::HandlerContext,
    helpers::{complete_burn_proof_to_contents, validate_burn_proof_file_name},
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::burn_proofs";

/// Extracts the public key prefix from a burn proof file name with format `{pubkey}_{commitment}.json`
/// (or legacy `{pubkey}-{commitment}.json`).
/// Returns `None` if the file name does not match this format.
fn extract_public_key_prefix(file_name: &str) -> Option<&str> {
    let stem = file_name.strip_suffix(".json")?;
    let (pk, commitment) = stem.split_once('_').or_else(|| stem.split_once('-'))?;
    if pk.is_empty() || commitment.is_empty() {
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

    let mut read_dir = match tokio::fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BurnProofsListResponse { proofs: Vec::new() });
        },
        Err(e) => {
            warn!(target: LOG_TARGET, "Failed to read burn proofs directory {}: {}", dir.display(), e);
            return Ok(BurnProofsListResponse { proofs: Vec::new() });
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

        let is_file = entry.file_type().await.map(|ft| ft.is_file()).unwrap_or(false);
        let path = entry.path();
        if !is_file || path.extension().is_some_and(|ext| ext != "json") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Apply account public key filter if provided
        if let Some(ref filter_pk) = req.filter_by_public_key {
            if let Some(file_pk) = extract_public_key_prefix(name) &&
                !file_pk.eq_ignore_ascii_case(filter_pk.as_str())
            {
                continue;
            }
        }

        // Read value from the proof file and verify embedded burn_public_key when filtering
        let mut matched_embedded_pk = true;
        let value = match tokio::fs::read(&path).await {
            Ok(bytes) => match serde_json::from_slice::<CompleteClaimBurnProof>(&bytes) {
                Ok(proof) => {
                    if let Some(ref filter_pk) = req.filter_by_public_key {
                        let proof_pk_hex = proof.claim_proof.burn_public_key.to_string();
                        if !proof_pk_hex.eq_ignore_ascii_case(filter_pk.as_str()) {
                            matched_embedded_pk = false;
                        }
                    }
                    Some(proof.claim_proof.value)
                },
                Err(e) => {
                    warn!(target: LOG_TARGET, "Failed to parse burn proof {}: {}", path.display(), e);
                    if req.filter_by_public_key.is_some() && extract_public_key_prefix(name).is_none() {
                        matched_embedded_pk = false;
                    }
                    None
                },
            },
            Err(e) => {
                warn!(target: LOG_TARGET, "Failed to read burn proof {}: {}", path.display(), e);
                if req.filter_by_public_key.is_some() && extract_public_key_prefix(name).is_none() {
                    matched_embedded_pk = false;
                }
                None
            },
        };
        if !matched_embedded_pk {
            continue;
        }

        proofs.push(BurnProofFileInfo {
            file_name: name.to_string(),
            value,
        });
    }

    proofs.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    Ok(BurnProofsListResponse { proofs })
}

pub async fn handle_get(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: BurnProofsGetRequest,
) -> Result<BurnProofsGetResponse, anyhow::Error> {
    context.authorize(token, &[Permission::BurnProofs(ReadOnly::Read)])?;
    let dir = context.config().get_burn_proof_dir(context.wallet_sdk().network());

    validate_burn_proof_file_name(&req.file_name)?;

    let file_path = dir.join(&req.file_name);
    let raw_bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|e| anyhow!("Failed to read burn proof file {}: {}", req.file_name, e))?;

    let complete_proof: CompleteClaimBurnProof = serde_json::from_slice(&raw_bytes)
        .map_err(|e| anyhow!("Failed to parse burn proof file {}: {}", req.file_name, e))?;

    let proof = complete_burn_proof_to_contents(complete_proof);

    Ok(BurnProofsGetResponse { proof })
}

#[cfg(test)]
mod tests {
    use super::extract_public_key_prefix;

    #[test]
    fn extract_public_key_prefix_supports_underscore_and_hyphen_formats() {
        assert_eq!(
            extract_public_key_prefix("0123456789abcdef_deadbeef.json"),
            Some("0123456789abcdef")
        );
        assert_eq!(
            extract_public_key_prefix("0123456789abcdef-deadbeef.json"),
            Some("0123456789abcdef")
        );
        assert_eq!(extract_public_key_prefix("custom.json"), None);
        assert_eq!(extract_public_key_prefix("_deadbeef.json"), None);
        assert_eq!(extract_public_key_prefix("pubkey_.json"), None);
    }
}
