//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use anyhow::anyhow;
use cucumber::{given, then, when};
use integration_tests::{claim_proof::CucumberClaimProof, util::cucumber_log};
use minotari_app_grpc::{
    tari_rpc,
    tari_rpc::{GetBalanceRequest, SubmitValidatorEvictionProofRequest, ValidateRequest},
};
use tari_engine_types::confidential::{AbridgedTransactionKernel, EncodedMerkleProof, MinotariBurnClaimProof};
use tari_ootle_wallet_sdk::apis::key_manager::KeyBranch;
use tari_template_lib::{
    models::EncryptedData,
    prelude::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, Scalar32Bytes, SchnorrSignatureBytes},
};
use tari_transaction_components::{
    tari_amount::T,
    transaction_components::{memo_field::TxType, MemoField},
};
use tari_wallet_daemon_client::types::ClaimBurnProof;
use tokio::time::sleep;

use crate::{spawn_minotari_wallet, TariWorld};

#[given(expr = "a wallet {word} connected to base node {word}")]
async fn start_wallet(world: &mut TariWorld, wallet_name: String, bn_name: String) {
    spawn_minotari_wallet(world, wallet_name, bn_name).await;
}

#[when(expr = "I burn {int}T on wallet {word} to proof {word} for wallet daemon {word}")]
async fn when_i_burn_on_wallet(
    world: &mut TariWorld,
    amount: u64,
    wallet_name: String,
    proof_name: String,
    walletd_name: String,
) {
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

    world.claim_proofs.insert(proof_name, CucumberClaimProof::Pending {
        commitment: PedersenCommitmentBytes::from_bytes(&resp.commitment).unwrap(),
        nonce_id: nonce.id,
    });
}

#[when(expr = "I wait for proof {word} to confirm on wallet {word}")]
#[allow(clippy::too_many_lines)]
async fn when_i_wait_for_proof_to_confirm_on_wallet(
    world: &mut TariWorld,
    proof_name: String,
    wallet_name: String,
) -> anyhow::Result<()> {
    let proof = world.claim_proofs.get(&proof_name).unwrap_or_else(|| {
        panic!("Claim proof {} not found", proof_name);
    });

    let CucumberClaimProof::Pending { commitment, nonce_id } = proof else {
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

        let Some(merkle_proof) = resp.merkle_proof else {
            cucumber_log("Proof not yet confirmed, waiting...");
            if remaining_attempts == 0 {
                panic!("Proof not confirmed after maximum ({ATTEMPTS}) attempts");
            }
            remaining_attempts -= 1;
            sleep(Duration::from_secs(1)).await;
            continue;
        };

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
                encoded_merkle_proof: EncodedMerkleProof {
                    block_hash: merkle_proof.block_hash.as_slice().try_into().map_err(|e| {
                        anyhow!(
                            "Block hash length {} is out of bounds: {e}",
                            merkle_proof.block_hash.len()
                        )
                    })?,
                    encoded_merkle_proof: merkle_proof
                        .encoded_proof
                        .try_into()
                        .map_err(|e| anyhow!("Encoded merkle proof length is out of bounds: {e}"))?,
                    leaf_index: merkle_proof.leaf_index,
                },
                kernel,
                value: resp.value,
            },
            owner_nonce_key_index: *nonce_id,
            encrypted_data: EncryptedData::try_from(resp.encrypted_data)
                .map_err(|e| anyhow!("Encrypted data length is out of bounds: {e}",))?,
        };

        world.claim_proofs.insert(proof_name, CucumberClaimProof::Confirmed {
            proof: Box::new(proof.clone()),
        });
        break;
    }

    Ok(())
}

#[when(expr = "wallet {word} has at least {int} {word}")]
pub async fn check_balance(world: &mut TariWorld, wallet_name: String, balance: u64, units: String) {
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
pub async fn submit_eviction(world: &mut TariWorld, eviction_name: String, wallet_name: String) {
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
