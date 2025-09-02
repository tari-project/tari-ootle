//  Copyright 2022 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use std::time::Duration;

use anyhow::Context;
use cucumber::{then, when};
use integration_tests::{util::cucumber_log, wallet_daemon_cli, TariWorld};
use tari_engine_types::commit_result::FinalizeResult;
use tari_ootle_wallet_sdk::apis::key_manager::KeyBranch;
use tari_template_lib::prelude::{crypto::CommitmentSignatureBytes, Amount, PedersenCommitmentBytes, Scalar32Bytes};
use tari_transaction_components::transaction_components::{memo_field::TxType, MemoField};
use tari_wallet_daemon_client::{
    types::{ClaimBurnProof, ExtClaimBurnProof},
    ComponentAddressOrName,
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
    proof_name: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    let result = claim_burn(world, proof_name, account_name, wallet_daemon_name)
        .await
        .unwrap();
    if let Some(ref reason) = result.any_reject() {
        panic!("Transaction failed: {}", reason);
    }
}

#[when(expr = "I claim burn {word} and spend it into account {word} using wallet daemon {word}, it fails")]
async fn when_i_claim_burn_via_wallet_daemon_it_fails(
    world: &mut TariWorld,
    proof_name: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    let _result = claim_burn(world, proof_name, account_name, wallet_daemon_name)
        .await
        .unwrap_err();

    // TODO: the wallet/indexer cannot find the substate before we submit the transaction, so this doesnt test the VN
    // behaviour. assert!(
    //     result.any_reject().is_some(),
    //     "Expected transaction to fail, but it succeeded"
    // );
}

#[when(expr = "I claim fees for validator {word} into account {word} using the wallet daemon {word}")]
async fn when_i_claim_fees_for_validator_and_epoch(
    world: &mut TariWorld,
    validator_node: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    let resp = wallet_daemon_cli::claim_fees(world, wallet_daemon_name, account_name, validator_node, false)
        .await
        .unwrap();
    resp.result.result.any_accept().unwrap_or_else(|| {
        panic!(
            "Expected fee claim to succeed but failed with {}",
            resp.result.result.fee_reject().unwrap()
        )
    });
}

#[when(expr = "I claim fees for validator {word} into account {word} using the wallet daemon {word}, it fails")]
async fn when_i_claim_fees_for_validator_and_epoch_fails(
    world: &mut TariWorld,
    validator_node: String,
    account_name: String,
    wallet_daemon_name: String,
) {
    let err = wallet_daemon_cli::claim_fees(world, wallet_daemon_name, account_name, validator_node, false)
        .await
        .unwrap_err();

    println!("Expected error: {}", err);
}

#[then(
    expr = "I make a confidential transfer with amount {int} from {word} to {word} creating output {word} via the \
            wallet_daemon {word}"
)]
async fn when_i_create_transfer_proof_via_wallet_daemon(
    world: &mut TariWorld,
    amount: u64,
    source_account_name: String,
    dest_account_name: String,
    outputs_name: String,
    wallet_daemon_name: String,
) {
    wallet_daemon_cli::transfer_confidential(
        world,
        source_account_name,
        dest_account_name,
        amount,
        wallet_daemon_name,
        outputs_name,
    )
    .await;
}

#[then(expr = "I create an account {word} via the wallet daemon {word}")]
#[when(expr = "I create an account {word} via the wallet daemon {word}")]
async fn when_i_create_account_via_wallet_daemon(
    world: &mut TariWorld,
    account_name: String,
    wallet_daemon_name: String,
) {
    wallet_daemon_cli::create_account(world, account_name, wallet_daemon_name).await;
}

#[then(expr = "I create an account {word} via the wallet daemon {word} with {int} free coins")]
#[when(expr = "I create an account {word} via the wallet daemon {word} with {int} free coins")]
async fn when_i_create_account_via_wallet_daemon_with_free_coins(
    world: &mut TariWorld,
    account_name: String,
    wallet_daemon_name: String,
    amount: i64,
) {
    wallet_daemon_cli::create_account_with_free_coins(world, account_name, wallet_daemon_name, amount.into(), None)
        .await;
}

#[when(expr = "I create a key named {word} for {word}")]
async fn when_i_create_a_wallet_key(world: &mut TariWorld, key_name: String, wallet_daemon_name: String) {
    let mut client = world.get_wallet_daemon(&wallet_daemon_name).get_authed_client().await;
    let key = client.create_key(KeyBranch::Account).await.unwrap();
    world.wallet_keys.insert(key_name, key.id);
}

#[then(expr = "I create an account {word} via the wallet daemon {word} with {int} free coins using key {word}")]
#[when(expr = "I create an account {word} via the wallet daemon {word} with {int} free coins using key {word}")]
async fn when_i_create_account_via_wallet_daemon_with_free_coins_using_key(
    world: &mut TariWorld,
    account_name: String,
    wallet_daemon_name: String,
    amount: i64,
    key_name: String,
) {
    wallet_daemon_cli::create_account_with_free_coins(
        world,
        account_name,
        wallet_daemon_name,
        amount.into(),
        Some(key_name),
    )
    .await;
}

#[when(expr = "I burn {int}T on wallet {word} for wallet daemon {word} into proof {word}")]
async fn when_i_burn_funds_with_wallet_daemon(
    world: &mut TariWorld,
    amount: u64,
    wallet_name: String,
    wallet_daemon_name: String,
    proof_name: String,
) {
    let mut wallet_daemon_client = wallet_daemon_cli::get_auth_wallet_daemon_client(world, &wallet_daemon_name).await;

    let nonce = wallet_daemon_client.create_key(KeyBranch::Nonce).await.unwrap();

    let public_key = nonce.public_key;
    cucumber_log("Burning funds using claim key {public_key}");

    let wallet = world
        .wallets
        .get(&wallet_name)
        .unwrap_or_else(|| panic!("Wallet {} not found", wallet_name));

    let mut client = wallet.create_client().await;
    let resp = client
        .create_burn_transaction(minotari_app_grpc::tari_rpc::CreateBurnTransactionRequest {
            amount: amount * 1_000_000,
            fee_per_gram: 1,
            payment_id: MemoField::open_from_string("Burn", TxType::Burn).to_bytes(),
            claim_public_key: public_key.to_vec(),
            sidechain_deployment_key: vec![],
        })
        .await
        .unwrap()
        .into_inner();

    assert!(resp.is_success, "Burn transaction failed: {}", resp.failure_message);

    let ownership_proof = resp.ownership_proof.as_ref().unwrap();
    let ownership_proof = CommitmentSignatureBytes::new(
        PedersenCommitmentBytes::from_bytes(&ownership_proof.public_nonce)
            .context("comsig public_nonce parse error")
            .unwrap(),
        Scalar32Bytes::from_bytes(&ownership_proof.u)
            .context("comsig u parse error")
            .unwrap(),
        Scalar32Bytes::from_bytes(&ownership_proof.v)
            .context("comsig v parse error")
            .unwrap(),
    );

    world.claim_proofs.insert(proof_name, ExtClaimBurnProof {
        claim_proof: ClaimBurnProof {
            reciprocal_claim_public_key: resp.reciprocal_claim_public_key.as_slice().try_into().unwrap(),
            commitment: resp.commitment.as_slice().try_into().unwrap(),
            ownership_proof,
            range_proof: resp.range_proof.try_into().unwrap(),
        },
        owner_nonce_key_index: nonce.id,
    });
}

#[when(regex = r"I check the balance of (\S+) on wallet daemon (\S+) the amount is (at )?(\S+) (\d+)")]
async fn check_account_balance_via_daemon(
    world: &mut TariWorld,
    account_name: String,
    wallet_daemon_name: String,
    _at: String,
    least_or_most: String,
    amount: i64,
) {
    // This also refreshes the wallet vaults
    let current_balance = wallet_daemon_cli::get_balance(world, &account_name, &wallet_daemon_name).await;
    match least_or_most.to_lowercase().as_str() {
        "least" => {
            if current_balance < amount {
                println!("Expected balance to be at least {} but was {}", amount, current_balance);
                panic!("Expected balance to be at least {} but was {}", amount, current_balance);
            }
        },
        "most" => {
            if current_balance > amount {
                println!("Expected balance to be at most {} but was {}", amount, current_balance);
                panic!("Expected balance to be at most {} but was {}", amount, current_balance);
            }
        },
        "exactly" => {
            if current_balance != amount {
                println!("Expected balance to be exactly {} but was {}", amount, current_balance);
                panic!("Expected balance to be exactly {} but was {}", amount, current_balance);
            }
        },

        _ => panic!("Expected 'at least', 'at most' or 'exactly', got {}", least_or_most),
    }
}

#[when(expr = "I wait for {word} on wallet daemon {word} to have balance {word} {int}")]
#[then(expr = "I wait for {word} on wallet daemon {word} to have balance {word} {int}")]
async fn wait_account_balance_via_daemon(
    world: &mut TariWorld,
    account_name: String,
    wallet_daemon_name: String,
    operator: String,
    amount: i64,
) {
    let op = match operator.as_str() {
        "gt" => |a, b| a > b,
        "gte" => |a, b| a >= b,
        "lt" => |a, b| a < b,
        "lte" => |a, b| a <= b,
        "eq" => |a, b| a == b,
        _ => panic!("Expected gt, gte, lt, lte or eq, got {}", operator),
    };

    let mut i = 0;
    loop {
        // This also refreshes the wallet vaults
        let current_balance = wallet_daemon_cli::get_balance(world, &account_name, &wallet_daemon_name).await;
        if op(current_balance, amount) {
            break;
        }

        i += 1;
        if i == 30 {
            panic!("Timeout waiting for balance. Current balance = {}", current_balance);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[when(expr = "I check the confidential balance of {word} on wallet daemon {word} the amount is at {word} {int}")]
async fn check_account_confidential_balance_is_via_daemon(
    world: &mut TariWorld,
    account_name: String,
    wallet_daemon_name: String,
    least_or_most: String,
    amount: i64,
) {
    // This also refreshes the wallet vaults
    let current_balance = wallet_daemon_cli::get_confidential_balance(world, account_name, wallet_daemon_name).await;
    match least_or_most.to_lowercase().as_str() {
        "least" => {
            if current_balance < amount {
                println!("Expected balance to be at least {} but was {}", amount, current_balance);
                panic!("Expected balance to be at least {} but was {}", amount, current_balance);
            }
        },
        "most" => {
            if current_balance > amount {
                println!("Expected balance to be at most {} but was {}", amount, current_balance);
                panic!("Expected balance to be at most {} but was {}", amount, current_balance);
            }
        },
        _ => panic!("Expected least or most, got {}", least_or_most),
    }
}

#[when(
    expr = "I transfer {int} tokens of resource {word} from account {word} to public key {word} via the wallet daemon \
            {word} named {word}"
)]
async fn when_transfer_via_wallet_daemon(
    world: &mut TariWorld,
    amount: i32,
    resource_address: String,
    account_name: String,
    destination_public_key: String,
    wallet_daemon_name: String,
    outputs_name: String,
) {
    let destination_public_key = *world.account_keys.get(&destination_public_key).unwrap();
    let amount = Amount::new(amount.into());

    let (resource_input_group, resource_name) = resource_address.split_once('/').unwrap_or_else(|| {
        panic!(
            "Resource address must be in the format '{{group}}/resources/{{index}}', got {}",
            resource_address
        )
    });
    let resource_address = world
        .outputs
        .get(resource_input_group)
        .unwrap_or_else(|| panic!("No outputs found with name {}", resource_input_group))
        .iter()
        .find(|(name, _)| **name == resource_name)
        .map(|(_, data)| data.clone())
        .unwrap_or_else(|| panic!("No resource named {}", resource_name))
        .substate_id
        .as_resource_address()
        .unwrap_or_else(|| panic!("{} is not a resource", resource_name));

    wallet_daemon_cli::transfer(
        world,
        account_name,
        destination_public_key,
        resource_address,
        amount,
        wallet_daemon_name,
        outputs_name,
    )
    .await;
}

#[when(
    expr = "I do a confidential transfer of {int} from account {word} to public key {word} via the wallet daemon \
            {word} named {word}"
)]
async fn when_confidential_transfer_via_wallet_daemon(
    world: &mut TariWorld,
    amount: u64,
    account_name: String,
    destination_public_key: String,
    wallet_daemon_name: String,
    outputs_name: String,
) {
    let destination_public_key = *world.account_keys.get(&destination_public_key).unwrap();

    wallet_daemon_cli::confidential_transfer(
        world,
        account_name,
        destination_public_key,
        amount.into(),
        wallet_daemon_name,
        outputs_name,
    )
    .await;
}

#[when(expr = "I set the default account for {word} to {word}")]
async fn when_i_set_the_default_account(world: &mut TariWorld, wallet_name: String, account_name: String) {
    let wallet = world
        .wallet_daemons
        .get(&wallet_name)
        .unwrap_or_else(|| panic!("No wallet daemon named {}", wallet_name));
    let mut client = wallet.get_authed_client().await;
    client
        .accounts_set_default(ComponentAddressOrName::Name(account_name))
        .await
        .unwrap();
}
