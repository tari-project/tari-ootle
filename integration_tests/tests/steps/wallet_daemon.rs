//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::{fs, time::Duration};

use cucumber::{gherkin::Step, then, when};
use integration_tests::{
    TariWorld,
    claim_proof::CucumberClaimProof,
    cucumber_log,
    util::transaction_builder,
    wallet_daemon_client,
};
use rand::{Rng, rngs::OsRng};
use tari_crypto::tari_utilities::ByteArray;
use tari_engine_types::commit_result::FinalizeResult;
use tari_ootle_transaction::args;
use tari_ootle_walletd_client::{
    ComponentAddressOrName,
    types::{TransactionSubmitRequest, TransactionWaitResultRequest},
};
use tari_template_lib_types::{Amount, bytes::Bytes, constants::TARI_TOKEN, crypto::PedersenCommitmentBytes};
use tari_transaction_components::{
    tari_amount::T,
    transaction_components::{MemoField, memo_field::TxType},
};

async fn claim_burn(
    world: &mut TariWorld,
    proof_name: String,
    account_name: String,
    wallet_daemon_name: String,
) -> anyhow::Result<FinalizeResult> {
    let claim_proof = world
        .claim_proofs
        .get(&proof_name)
        .unwrap_or_else(|| panic!("Burn proof {} not found", proof_name));
    let claim_proof = claim_proof
        .confirmed()
        .unwrap_or_else(|| panic!("Burn proof {} is not confirmed, cannot claim burn", proof_name));

    cucumber_log!("Claiming burn with proof: {:?}", claim_proof);
    let walletd = world.get_wallet_daemon(&wallet_daemon_name);
    // Then burn into the new account
    let claim_burn_resp = walletd.claim_burn(&account_name, claim_proof.clone()).await?;
    let resp = walletd
        .wait_for_transaction_result(claim_burn_resp.transaction_id)
        .await;
    assert!(!resp.timed_out, "Timed out waiting for claim burn transaction result");
    Ok(resp.result.expect("transaction result is None when claiming burn"))
}

#[when(expr = "I claim burn {word} and spend it into account {word} using wallet daemon {word}")]
#[then(expr = "I claim burn {word} and spend it into account {word} using wallet daemon {word}")]
async fn when_i_claim_burn_via_wallet_daemon(
    world: &mut TariWorld,
    step: &Step,
    proof_name: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let result = claim_burn(world, proof_name, account_name, wallet_daemon_name)
        .await
        .unwrap();
    if let Some(ref reason) = result.any_reject() {
        cucumber_log!("Transaction failed: {}", reason);
        panic!("Transaction failed: {}", reason);
    }
}

#[when(expr = "I claim burn {word} and spend it into account {word} using wallet daemon {word}, it fails")]
async fn when_i_claim_burn_via_wallet_daemon_it_fails(
    world: &mut TariWorld,
    step: &Step,
    proof_name: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let result = claim_burn(world, proof_name, account_name, wallet_daemon_name)
        .await
        .unwrap();

    assert!(
        result.any_reject().is_some(),
        "Expected transaction to fail, but it succeeded: {:?}",
        result
    );
}

#[when(expr = "I claim fees for validator {word} into account {word} using the wallet daemon {word}")]
async fn when_i_claim_fees_for_validator_and_epoch(
    world: &mut TariWorld,
    step: &Step,
    validator_node: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let resp = wallet_daemon_client::claim_fees(world, wallet_daemon_name, account_name, validator_node, false)
        .await
        .unwrap();
    resp.result.result.any_accept().unwrap_or_else(|| {
        panic!(
            "Expected fee claim to succeed but failed with {}",
            resp.result.result.fee_reject().unwrap()
        )
    });
}

#[then(expr = "I run up {amount} in fees via wallet daemon {word}")]
async fn when_i_run_up_fees_via_wallet_daemon(
    world: &mut TariWorld,
    step: &Step,
    amount: u64,
    wallet_daemon_name: String,
) {
    cucumber_log!("==== Step: {}", step.value);
    let walletd = world.get_wallet_daemon(&wallet_daemon_name);
    let resp = walletd.run_up_fees(amount).await.unwrap();
    assert!(resp.success, "Failed to run up fees");
}