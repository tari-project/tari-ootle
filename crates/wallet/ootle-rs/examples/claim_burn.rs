//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Example: claim a Layer 1 (minotari) burn into an Ootle account.
//!
//! Claiming a burn is a chicken-and-egg process: the L1 burn must be addressed to a *claim public
//! key* derived from the claiming account's secret, so you must know the account before you can
//! produce the proof. This example uses a fixed, deterministic secret so the claim key is stable
//! across runs.
//!
//! WARNING: the secret is hard-coded for localnet demonstration only — anyone can derive it and
//! claim funds burned to its claim key. Never use it for real funds.
//!
//! Run with no arguments to print the claim public key:
//!
//! ```bash
//! cargo run -p ootle-rs --example claim_burn
//! ```
//!
//! Burn tTARI on the L1 (minotari) wallet to that claim key (e.g. `create_burn_transaction` with
//! `claim_public_key = <printed key>`). Once mined, fetch the proof (`get_burn_claim_proof`) and
//! save it as JSON in the wallet daemon's `ClaimBurnProofContents` shape:
//! `{ "claim_proof": { ... }, "encrypted_data": "..." }`. Then re-run with the proof file (this
//! step requires a running indexer):
//!
//! ```bash
//! cargo run -p ootle-rs --example claim_burn -- ./burn_proof.json
//! ```

use std::{env, fs};

use ootle_rs::{
    Network,
    TransactionRequest,
    claim_burn::{ClaimBurn, MinotariBurnClaimProof},
    default_indexer_url,
    key_provider::PrivateKeyProvider,
    keys::OotleSecretKey,
    provider::{Provider, ProviderBuilder},
    template_types::EncryptedData,
    wallet::{NetworkWallet, OotleWallet},
};
use tari_crypto::{
    keys::{PublicKey, SecretKey},
    ristretto::{RistrettoPublicKey, RistrettoSecretKey},
    tari_utilities::hex::Hex,
};

/// MicroTARI revealed from the claimed funds to pay the transaction fee.
const MAX_FEE: u64 = 2000;

/// The L2 burn proof contents, as produced for the burned output. This matches the wallet daemon's
/// `ClaimBurnProofContents` serde shape.
#[derive(serde::Deserialize)]
struct BurnProofFile {
    claim_proof: MinotariBurnClaimProof,
    encrypted_data: EncryptedData,
}

#[tokio::main]
async fn main() {
    // The network the funds were burned to. Change this to match your deployment.
    let network = Network::LocalNet;

    // Fixed, deterministic example keys so the claim public key is stable across runs.
    // DO NOT use these for real funds — see the module warning.
    let account_secret = RistrettoSecretKey::from_uniform_bytes(&[0x11u8; 64]).expect("valid scalar");
    let view_only_secret = RistrettoSecretKey::from_uniform_bytes(&[0x22u8; 64]).expect("valid scalar");

    let secret = OotleSecretKey::new(network, account_secret.clone(), view_only_secret);
    let wallet = OotleWallet::from(PrivateKeyProvider::new(secret));

    // The L1 burn must be addressed to this claim public key (the account's public key).
    let claim_public_key = RistrettoPublicKey::from_secret_key(&account_secret);
    println!("Account address:   {}", wallet.default_address());
    println!("Claim public key:  {}", claim_public_key.to_hex());

    let Some(proof_path) = env::args().nth(1) else {
        println!();
        println!("No burn proof supplied. To claim a burn:");
        println!("  1. Burn tTARI on the L1 (minotari) wallet to the claim public key above");
        println!("     (create_burn_transaction with claim_public_key = the key above).");
        println!("  2. Once mined, fetch the proof (get_burn_claim_proof) and save it as JSON:");
        println!("       {{ \"claim_proof\": {{ ... }}, \"encrypted_data\": \"...\" }}");
        println!("  3. Re-run with the proof file:");
        println!("       cargo run -p ootle-rs --example claim_burn -- ./burn_proof.json");
        return;
    };

    let BurnProofFile {
        claim_proof,
        encrypted_data,
    } = serde_json::from_str(&fs::read_to_string(&proof_path).expect("failed to read burn proof file"))
        .expect("failed to parse burn proof JSON");

    let mut provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect(default_indexer_url(network))
        .await
        .expect("failed to connect to indexer");
    assert_eq!(provider.network(), network);

    // Build the claim transaction and the sealer that signs it with the derived stealth claim key.
    let (unsigned_tx, sealer) = ClaimBurn::new(&provider, claim_proof, encrypted_data)
        .with_max_fee(MAX_FEE)
        .with_memo_message("claimed via ootle-rs")
        .prepare()
        .await
        .expect("failed to prepare burn claim");

    // Estimate fees with a dry run first (optional).
    let dry_run = provider
        .sign_and_send_dry_run_with(&sealer, unsigned_tx.clone())
        .await
        .expect("dry run submission failed");
    dry_run.expect_success();
    println!(
        "Dry run OK. Estimated fees: {}",
        dry_run.finalize.fee_receipt.total_fees_charged()
    );

    // Submit the claim for real.
    let tx = TransactionRequest::default()
        .with_transaction(unsigned_tx)
        .build(&sealer)
        .await
        .expect("failed to seal claim transaction");

    let pending = provider.send_transaction(tx).await.expect("failed to submit claim");
    println!("⌛️ Claim pending... {}", pending.tx_id());
    let outcome = pending.watch().await.expect("claim transaction failed");
    println!("✅ Burn claimed! Outcome: {:?}", outcome);
}
