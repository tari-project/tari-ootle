//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use anyhow::anyhow;
use cucumber::{gherkin::Step, given, then, when};
use integration_tests::{claim_proof::CucumberClaimProof, cucumber_log};
use minotari_app_grpc::{
    tari_rpc,
    tari_rpc::{GetBalanceRequest, SubmitValidatorEvictionProofRequest, ValidateRequest},
};
use serde_json;
use tari_engine_types::confidential::{AbridgedTransactionKernel, EncodedMerkleProof, MinotariBurnClaimProof};
use tari_ootle_wallet_sdk::models::KeyBranch;
use tari_template_lib::{
    prelude::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, Scalar32Bytes, SchnorrSignatureBytes},
    types::EncryptedData,
};
use tari_transaction_components::{
    tari_amount::T,
    transaction_components::{memo_field::TxType, MemoField},
};
use tari_wallet_daemon_client::types::ClaimBurnProof;
use tokio::time::sleep;

use crate::{spawn_minotari_wallet, TariWorld};

#[given(expr = "a wallet {word} connected to base node {word}")]
async fn start_wallet(world: &mut TariWorld, step: &Step, wallet_name: String, bn_name: String) {
    cucumber_log!("==== Step: {}", step.value);
    spawn_minotari_wallet(world, wallet_name, bn_name).await;
}

#[when(expr = "I burn {int}T on wallet {word} to proof {word} for wallet daemon {word}")]
async fn when_i_burn_on_wallet(
    world: &mut TariWorld,
    step: &Step,
    amount: u64,
    wallet_name: String,
    proof_name: String,
    walletd_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let wallet = world
        .wallets
        .get(&wallet_name)
        .unwrap_or_else(|| panic!("Wallet {} not found", wallet_name));

    let walletd = world.get_wallet_daemon(&walletd_name);
    let mut client = walletd.get_authed_client().await;
    let nonce = client.create_key(KeyBranch::Nonce).await.unwrap();

    let amount = amount * T;
    let mut client = wallet.create_client().await;
    let resp = client
        .create_burn_transaction(minotari_app_grpc::tari_rpc::CreateBurnTransactionRequest {
            amount: amount.as_u64(),
            fee_per_gram: 1,
            payment_id: MemoField::new_open("Burn".as_bytes().to_vec(), TxType::Burn)
                .unwrap()
                .to_bytes(),
            claim_public_key: nonce.public_key.as_bytes().to_vec(),
            sidechain_deployment_key: vec![],
        })
        .await
        .unwrap()
        .into_inner();

    assert!(resp.is_success);

    // Extract kernel signature datai
    let kernel_excess_sig_nonce = resp.kernel_excess_nonce.clone();
    let kernel_excess_sig_signature = resp.kernel_excess_signature.clone();

    integration_tests::cucumber_log!(
        "Burn transaction created with kernel_excess_sig nonce: {}, signature: {}",
        hex::encode(&kernel_excess_sig_nonce),
        hex::encode(&kernel_excess_sig_signature)
    );

    // save the excess to world for retrieval later.

    // world.set_data(format!("proof_{}_excess", proof_name), hex::encode(&kernel_excess_sig_nonce));
    // world.set_data(format!("proof_{}_sig", proof_name), hex::encode(&kernel_excess_sig_signature));

    // Get the base node connected to this wallet to call the HTTP endpoint
    // let base_node_name = world
    //     .wallets
    //     .iter()
    //     .find(|(name, _)| *name == &wallet_name)
    //     .and_then(|(_, wallet_process)| {
    //         // Find which base node this wallet is connected to by checking spawn parameters
    //         // For now, we'll use the first base node
    //         world.base_nodes.keys().next().cloned()
    //     })
    //     .expect("No base node found");

    // let base_node = world
    //     .base_nodes
    //     .get(&base_node_name)
    //     .unwrap_or_else(|| panic!("Base node {} not found", base_node_name));

    // // Call the base node HTTP endpoint to get kernel merkle proof
    // let http_client = reqwest::Client::new();
    // let url = format!(
    //     "http://127.0.0.1:{}/generate_kernel_merkle_proof?excess_sig_public_nonce={}&excess_sig_signature={}",
    //     base_node.http_port,
    //     hex::encode(&kernel_excess_sig_nonce),
    //     hex::encode(&kernel_excess_sig_signature)
    // );

    // integration_tests::cucumber_log!("Calling base node HTTP endpoint: {}", url);

    // // Try to get the kernel proof (it may not be available yet if not mined)
    // match http_client.get(&url).send().await {
    //     Ok(response) => {
    //         if response.status().is_success() {
    //             let proof_response = response.text().await.unwrap();
    //             integration_tests::cucumber_log!("Kernel merkle proof response: {}", proof_response);
    //         } else {
    //             integration_tests::cucumber_log!(
    //                 "Kernel merkle proof not yet available (status: {}). This is expected if the transaction hasn't been mined yet.",
    //                 response.status()
    //             );
    //         }
    //     },
    //     Err(e) => {
    //         integration_tests::cucumber_log!("Failed to call kernel merkle proof endpoint: {}", e);
    //     },
    // }

    world.claim_proofs.insert(
        proof_name,
        CucumberClaimProof::Pending {
            commitment: PedersenCommitmentBytes::from_bytes(&resp.commitment).unwrap(),
            nonce_id: nonce.id,
            kernel_excess_sig_nonce,
            kernel_excess_sig_signature,
        },
    );
}

#[when(expr = "I wait for proof {word} to confirm on wallet {word}")]
#[allow(clippy::too_many_lines)]
async fn when_i_wait_for_proof_to_confirm_on_wallet(
    world: &mut TariWorld,
    step: &Step,
    proof_name: String,
    wallet_name: String,
) -> anyhow::Result<()> {
    cucumber_log!("==== Step: {}", step.value);
    let proof = world.claim_proofs.get(&proof_name).unwrap_or_else(|| {
        panic!("Claim proof {} not found", proof_name);
    });

    let CucumberClaimProof::Pending {
        commitment,
        nonce_id,
        kernel_excess_sig_nonce,
        kernel_excess_sig_signature,
    } = proof
    else {
        // Already confirmed
        return Ok(());
    };

    let wallet = world
        .wallets
        .get(&wallet_name)
        .unwrap_or_else(|| panic!("Wallet {} not found", wallet_name));

    let mut client = wallet.create_client().await;

    const ATTEMPTS: usize = 60;
    let mut remaining_attempts = ATTEMPTS;
    loop {
        let resp = client
            .get_burn_claim_proof(tari_rpc::GetBurnClaimProofRequest {
                commitment: commitment.as_bytes().to_vec(),
            })
            .await
            .unwrap()
            .into_inner();

        // let Some(merkle_proof) = resp.merkle_proof else {
        //     integration_tests::cucumber_log!("Proof not yet confirmed, waiting...");
        //     if remaining_attempts == 0 {
        //         panic!("Proof not confirmed after maximum ({ATTEMPTS}) attempts");
        //     }
        //     remaining_attempts -= 1;
        //     sleep(Duration::from_secs(1)).await;
        //     continue;
        // };

        // Now that the proof is confirmed, call the base node HTTP endpoint to get kernel merkle proof
        integration_tests::cucumber_log!(
            "Proof confirmed! Now calling base node HTTP endpoint to get kernel merkle proof"
        );

        let base_node = world.base_nodes.values().next().expect("No base node found");

        let http_client = reqwest::Client::new();
        let url = format!(
            "http://127.0.0.1:{}/generate_kernel_merkle_proof?excess_sig_public_nonce={}&excess_sig_signature={}",
            base_node.http_port,
            hex::encode(kernel_excess_sig_nonce),
            hex::encode(kernel_excess_sig_signature)
        );

        integration_tests::cucumber_log!("Calling base node HTTP endpoint: {}", url);

        let mut merkle_proof: Option<EncodedMerkleProof> = None;
        match http_client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    let proof_response = response.text().await.unwrap();
                    integration_tests::cucumber_log!("SUCCESS! Kernel merkle proof response: {}", proof_response);
                    merkle_proof = Some(
                        serde_json::from_str(&proof_response)
                            .map_err(|e| anyhow!("Failed to deserialize merkle proof: {e}"))?,
                    );
                } else {
                    let status = response.status();
                    let error_text = response.text().await.unwrap_or_default();
                    integration_tests::cucumber_log!(
                        "WARNING: Failed to get kernel merkle proof (status: {}): {}",
                        status,
                        error_text
                    );
                }
            },
            Err(e) => {
                integration_tests::cucumber_log!("ERROR: Failed to call kernel merkle proof endpoint: {}", e);
            },
        }
        if merkle_proof.is_none() {
            panic!("Kernel merkle proof not found");
        }
        let merkle_proof = merkle_proof.unwrap();

        let claim_proof = resp.claim_proof.ok_or_else(|| anyhow!("No claim proof in response"))?;
        let ownership_proof = claim_proof
            .ownership_proof
            .ok_or_else(|| anyhow!("No ownership proof in response"))?;
        let commitment = PedersenCommitmentBytes::from_bytes(&claim_proof.commitment)
            .map_err(|e| anyhow!("commitment parse error: {e}"))?;

        let ownership_proof = SchnorrSignatureBytes::new(
            RistrettoPublicKeyBytes::from_bytes(&ownership_proof.public_nonce)
                .map_err(|e| anyhow!("sig public_nonce parse error {e}"))?,
            Scalar32Bytes::from_bytes(&ownership_proof.signature).map_err(|e| anyhow!("sig parse error {e}"))?,
        );

        let reciprocal_claim_public_key = RistrettoPublicKeyBytes::from_bytes(&claim_proof.reciprocal_claim_public_key)
            .map_err(|e| anyhow!("reciprocal_claim_public_key parse error {e}"))?;
        let kernel = resp.kernel.ok_or_else(|| anyhow!("No kernel in response"))?;
        let kernel = AbridgedTransactionKernel {
            version: kernel.version as u8,
            fee: kernel.fee,
            lock_height: kernel.lock_height,
            excess: kernel
                .excess
                .as_slice()
                .try_into()
                .map_err(|e| anyhow!("excess parse error: {e}"))?,
            excess_sig: {
                let excess_sig = kernel
                    .excess_sig
                    .as_ref()
                    .ok_or_else(|| anyhow!("No excess_sig in response"))?;

                SchnorrSignatureBytes::new(
                    excess_sig
                        .public_nonce
                        .as_slice()
                        .try_into()
                        .map_err(|e| anyhow!("excess_sig parse error: {e}"))?,
                    excess_sig
                        .signature
                        .as_slice()
                        .try_into()
                        .map_err(|e| anyhow!("excess_sig parse error: {e}"))?,
                )
            },
        };

        let proof = ClaimBurnProof {
            claim_proof: MinotariBurnClaimProof {
                burn_public_key: reciprocal_claim_public_key,
                commitment,
                ownership_proof,
                encoded_merkle_proof: merkle_proof,
                kernel,
                value: resp.value,
            },
            owner_nonce_key_index: *nonce_id,
            encrypted_data: EncryptedData::try_from(resp.encrypted_data)
                .map_err(|e| anyhow!("Encrypted data length is out of bounds: {e}",))?,
        };

        world.claim_proofs.insert(
            proof_name,
            CucumberClaimProof::Confirmed {
                proof: Box::new(proof.clone()),
            },
        );
        break;
    }

    Ok(())
}

#[when(expr = "wallet {word} has at least {int} {word}")]
pub async fn check_balance(world: &mut TariWorld, step: &Step, wallet_name: String, balance: u64, units: String) {
    cucumber_log!("==== Step: {}", step.value);
    const MAX_WAIT_TIME_SECS: u64 = 100;
    let wallet = world
        .wallets
        .get(&wallet_name)
        .unwrap_or_else(|| panic!("Wallet {} not found", wallet_name));

    let mut client = wallet.create_client().await;
    let mut iterations = 0;
    let balance = match units.as_str() {
        "T" => balance * 1_000_000,
        "uT" => balance,
        _ => panic!("Unknown unit {}", units),
    };

    loop {
        let _result = client.validate_all_transactions(ValidateRequest {}).await.unwrap();
        let resp = client
            .get_balance(GetBalanceRequest { payment_id: None })
            .await
            .unwrap()
            .into_inner();
        if resp.available_balance >= balance {
            break;
        }
        eprintln!(
            "Waiting for wallet {} to have at least {} uT (balance: {} uT, pending: {} uT)",
            wallet_name, balance, resp.available_balance, resp.pending_incoming_balance
        );
        sleep(Duration::from_secs(2)).await;

        if iterations == MAX_WAIT_TIME_SECS.div_ceil(2) {
            panic!(
                "Wallet {} did not have at least {} uT after {} seconds  (balance: {} uT, pending: {} uT)",
                wallet_name, balance, MAX_WAIT_TIME_SECS, resp.available_balance, resp.pending_incoming_balance
            );
        }
        iterations += 1;
    }
}

#[then(expr = "I submit the eviction proof {word} to {word}")]
#[when(expr = "I submit the eviction proof {word} to {word}")]
pub async fn submit_eviction(world: &mut TariWorld, step: &Step, eviction_name: String, wallet_name: String) {
    cucumber_log!("==== Step: {}", step.value);
    let eviction = world
        .eviction_proofs
        .get(&eviction_name)
        .unwrap_or_else(|| panic!("Eviction proof {} not found", eviction_name));
    let wallet = world.get_wallet(&wallet_name);
    let mut client = wallet.create_client().await;
    client
        .submit_validator_eviction_proof(SubmitValidatorEvictionProofRequest {
            proof: Some(eviction.into()),
            fee_per_gram: 1,
            message: "Eviction proof in cucumber".to_string(),
            sidechain_deployment_key: vec![],
        })
        .await
        .unwrap();
}
