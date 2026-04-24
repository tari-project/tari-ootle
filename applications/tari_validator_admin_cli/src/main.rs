//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! # tari_validator_admin_cli
//!
//! Operator tool for issuing break-glass consensus directives to one or more validator
//! nodes via their admin JSON-RPC endpoints.
//!
//! Every directive is signed with the configured governance secret key. The signature is
//! what authenticates the directive against the validator's `governance_public_key` config.
//! The admin RPC address binding is defence-in-depth, not the primary auth.
//!
//! ## Usage
//!
//! ```text
//! tari_validator_admin_cli rollback \
//!     --target-epoch 42 \
//!     --governance-key-file /path/to/gov.secret \
//!     --validator-rpc http://127.0.0.1:5000/json_rpc \
//!     [--validator-rpc http://validator-2:5000/json_rpc ...]
//! ```

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use rand::rngs::OsRng;
use serde_json::json;
use tari_consensus_types::{ConsensusDirective, DirectiveBody, DirectiveKind};
use tari_crypto::{
    keys::PublicKey,
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_ootle_common_types::Epoch;
use tari_validator_node_client::types::{ApplyConsensusDirectiveRequest, ApplyConsensusDirectiveResponse};

#[derive(Parser, Debug)]
#[clap(
    author,
    version,
    about = "Issue break-glass consensus directives to Tari validator nodes"
)]
struct Cli {
    #[clap(subcommand)]
    command: AdminCommand,
}

#[derive(Subcommand, Debug)]
enum AdminCommand {
    /// Roll validators back to an epoch checkpoint they already hold locally.
    Rollback {
        /// Target epoch to roll back to. Validators must have a locally-stored
        /// EpochCheckpoint for this epoch; the CLI does not ship a checkpoint.
        #[clap(long)]
        target_epoch: u64,

        /// Path to a file containing the governance secret key bytes (32 bytes, hex-encoded
        /// or raw). Used to sign the directive.
        #[clap(long)]
        governance_key_file: PathBuf,

        /// One or more validator admin-RPC URLs. The CLI will post to each in sequence and
        /// report per-validator outcomes. For quorum-safe rollout, pass every validator in
        /// the committee.
        #[clap(long, required = true)]
        validator_rpc: Vec<String>,

        /// Override the nonce embedded in the directive body. Defaults to a fresh random
        /// 64-bit value so operator retries don't collide with a prior emission.
        #[clap(long)]
        nonce: Option<u64>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        AdminCommand::Rollback {
            target_epoch,
            governance_key_file,
            validator_rpc,
            nonce,
        } => run_rollback(target_epoch, governance_key_file, validator_rpc, nonce).await,
    }
}

async fn run_rollback(
    target_epoch: u64,
    governance_key_file: PathBuf,
    validator_rpc: Vec<String>,
    nonce: Option<u64>,
) -> Result<()> {
    let gov_secret = load_secret_key(&governance_key_file)
        .with_context(|| format!("loading governance secret key from {}", governance_key_file.display()))?;
    let gov_public = RistrettoPublicKey::from_secret_key(&gov_secret);
    eprintln!("Using governance public key: {}", hex::encode(gov_public.as_bytes()));

    let nonce = nonce.unwrap_or_else(rand::random);
    let issued_at_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let body = DirectiveBody {
        kind: DirectiveKind::rollback_to_epoch(Epoch(target_epoch)),
        nonce,
        issued_at_unix_secs,
    };
    let directive =
        ConsensusDirective::sign(body, &gov_secret, &mut OsRng).map_err(|e| anyhow!("signing directive: {e}"))?;

    let directive_id = directive.id();
    eprintln!("Directive ID: {}", directive_id);
    eprintln!("Target epoch: {target_epoch}, nonce: {nonce}");

    let directive_bytes = borsh::to_vec(&directive).context("serialising directive")?;
    let directive_hex = hex::encode(&directive_bytes);

    let client = reqwest::Client::new();
    let mut any_failed = false;

    for url in &validator_rpc {
        eprintln!("\n→ {url}");
        match submit_to(&client, url, &directive_hex).await {
            Ok(resp) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&resp).unwrap_or_else(|_| format!("{:?}", resp)),
                );
            },
            Err(err) => {
                any_failed = true;
                eprintln!("  ✗ {err:#}");
            },
        }
    }

    if any_failed {
        bail!("one or more validators rejected or errored on the directive");
    }

    Ok(())
}

async fn submit_to(
    client: &reqwest::Client,
    url: &str,
    directive_hex: &str,
) -> Result<ApplyConsensusDirectiveResponse> {
    let req = ApplyConsensusDirectiveRequest {
        directive_hex: directive_hex.to_string(),
    };
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "admin.apply_consensus_directive",
        "params": req,
    });

    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;

    let status = resp.status();
    let value: serde_json::Value = resp.json().await.with_context(|| format!("decoding JSON from {url}"))?;

    if !status.is_success() {
        bail!("HTTP {status}: {value}");
    }

    if let Some(err) = value.get("error") {
        bail!("JSON-RPC error: {err}");
    }

    let result = value
        .get("result")
        .ok_or_else(|| anyhow!("response has neither result nor error: {value}"))?;

    let parsed: ApplyConsensusDirectiveResponse = serde_json::from_value(result.clone())
        .with_context(|| format!("decoding ApplyConsensusDirectiveResponse from {value}"))?;
    Ok(parsed)
}

/// Load a 32-byte secret key from a file. Accepts either raw 32 bytes or a hex-encoded
/// representation (64 hex chars, optionally with 0x prefix and/or trailing whitespace).
fn load_secret_key(path: &PathBuf) -> Result<RistrettoSecretKey> {
    let raw = fs::read(path).with_context(|| format!("reading {}", path.display()))?;

    // Try as raw 32 bytes first.
    if raw.len() == 32 {
        return RistrettoSecretKey::from_canonical_bytes(&raw)
            .map_err(|e| anyhow!("key file is 32 bytes but not a canonical Ristretto scalar: {e:?}"));
    }

    // Treat as text: strip whitespace, optional 0x.
    let text = std::str::from_utf8(&raw).context("key file is neither 32 raw bytes nor valid UTF-8")?;
    let cleaned = text.trim().trim_start_matches("0x");
    let bytes = hex::decode(cleaned).context("key file text is not valid hex")?;
    if bytes.len() != 32 {
        bail!("decoded key length is {} bytes, expected 32", bytes.len());
    }
    RistrettoSecretKey::from_canonical_bytes(&bytes)
        .map_err(|e| anyhow!("decoded key is not a canonical Ristretto scalar: {e:?}"))
}
