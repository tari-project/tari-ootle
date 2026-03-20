//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fs, path::Path};

use anyhow::anyhow;
use axum_extra::headers::authorization::Bearer;
use log::*;
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{
        BurnProofFileInfo,
        BurnProofsGetRequest,
        BurnProofsGetResponse,
        BurnProofsListRequest,
        BurnProofsListResponse,
    },
};
use tari_sidechain::CompleteClaimBurnProof;

use super::{context::HandlerContext, helpers::complete_burn_proof_to_contents};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::burn_proofs";

/// Extracts the public key prefix from a burn proof file name with format `{pubkey}_{commitment}.json`.
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
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let dir = context.config().get_burn_proof_dir(context.wallet_sdk().network());

    let entries = match fs::read_dir(&dir) {
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
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!(target: LOG_TARGET, "Failed to read directory entry: {}", e);
                continue;
            },
        };

        let path = entry.path();
        if !path.is_file() || path.extension().is_some_and(|ext| ext != "json") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Apply account public key filter if provided
        if let Some(ref filter_pk) = req.filter_by_public_key {
            // If the file name matches the expected format, check if the key matches.
            // If the file name doesn't match the format, always include it.
            if let Some(file_pk) = extract_public_key_prefix(name) &&
                file_pk != filter_pk.as_str()
            {
                continue;
            }
        }

        // Read value from the proof file
        let value = match fs::File::open(&path).and_then(|f| {
            serde_json::from_reader::<_, CompleteClaimBurnProof>(&f)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        }) {
            Ok(proof) => Some(proof.claim_proof.value),
            Err(e) => {
                warn!(target: LOG_TARGET, "Failed to read burn proof {}: {}", name, e);
                None
            },
        };

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
    context.check_auth(token, &[JrpcPermission::Admin])?;
    let dir = context.config().get_burn_proof_dir(context.wallet_sdk().network());

    // Prevent path traversal
    let file_name = Path::new(&req.file_name);
    if file_name.components().count() != 1 || req.file_name.contains("..") {
        return Err(anyhow!("Invalid file name"));
    }

    let path = dir.join(&req.file_name);
    let file = fs::File::open(&path).map_err(|e| {
        warn!(target: LOG_TARGET, "Failed to open burn proof file {}: {}", path.display(), e);
        anyhow!("Burn proof file not found: {}", req.file_name)
    })?;

    let complete_proof: CompleteClaimBurnProof = serde_json::from_reader(&file).map_err(|e| {
        warn!(target: LOG_TARGET, "Failed to parse burn proof file {}: {}", path.display(), e);
        anyhow!("Invalid burn proof file: {}", req.file_name)
    })?;

    let proof = complete_burn_proof_to_contents(complete_proof)?;

    Ok(BurnProofsGetResponse { proof })
}

/// Moves a burn proof file into the `claimed` subdirectory of the burn proof directory.
/// This is called after a claim transaction has been successfully submitted.
pub fn mark_as_claimed(burn_proof_dir: &Path, file_name: &str) {
    let claimed_dir = burn_proof_dir.join("claimed");
    if let Err(e) = fs::create_dir_all(&claimed_dir) {
        warn!(
            target: LOG_TARGET,
            "Failed to create claimed directory {}: {}",
            claimed_dir.display(),
            e
        );
        return;
    }

    let src = burn_proof_dir.join(file_name);
    let dst = claimed_dir.join(file_name);
    if let Err(e) = fs::rename(&src, &dst) {
        warn!(
            target: LOG_TARGET,
            "Failed to move claimed burn proof {} -> {}: {}",
            src.display(),
            dst.display(),
            e
        );
    } else {
        info!(
            target: LOG_TARGET,
            "Burn proof {} marked as claimed",
            file_name
        );
    }
}
